mod support;

use async_trait::async_trait;
use backend::{
    bot_commands::BotMessage,
    dao::{
        CategoryDao, DailyDao, PgDailyDao, TaskDao,
        morning::{Decision, MorningStore},
    },
    domain::{Subtask, TaskUrgency},
    morning::{BotCallback, BotUpdate, MorningBot, MorningWorker},
    service::{MessageSender, NotificationError},
};
use std::sync::{Arc, Mutex};
use support::{TELEGRAM_ID, TestDb};

async fn task(db: &TestDb, description: &str) -> i64 {
    let category = db.categories().create("Work").await.unwrap();
    db.tasks()
        .create(category.id, description, TaskUrgency::Soon)
        .await
        .unwrap()
        .id
}

fn daily(db: &TestDb) -> PgDailyDao {
    PgDailyDao::new(db.pool.clone(), db.user.id)
}

async fn expire(db: &TestDb) {
    sqlx::query("UPDATE daily_tasks SET expires_at = statement_timestamp() - interval '1 minute'")
        .execute(&db.pool)
        .await
        .unwrap();
}

async fn due(db: &TestDb) -> i64 {
    sqlx::query_scalar("UPDATE morning_offers SET due_at = statement_timestamp() - interval '1 second', valid_until = statement_timestamp() + interval '1 hour', next_attempt_at = statement_timestamp() RETURNING id")
        .fetch_one(&db.pool).await.unwrap()
}

#[tokio::test]
async fn morning_schedule_uses_local_calendar_across_dst_and_fractional_offsets() {
    let db = TestDb::new().await;
    for (zone, reset, due, until) in [
        (
            "America/New_York",
            "2026-03-08T07:00:00Z",
            "2026-03-08T12:00:00Z",
            "2026-03-09T06:00:00Z",
        ),
        (
            "America/New_York",
            "2026-10-31T06:00:00Z",
            "2026-10-31T12:00:00Z",
            "2026-11-01T07:00:00Z",
        ),
        (
            "Asia/Kathmandu",
            "2026-10-28T20:15:00Z",
            "2026-10-29T02:15:00Z",
            "2026-10-29T20:15:00Z",
        ),
    ] {
        let correct: bool = sqlx::query_scalar("SELECT due_at = $3::text::timestamptz AND valid_until = $4::text::timestamptz FROM morning_offer_schedule($1, $2::text::timestamptz)")
            .bind(zone).bind(reset).bind(due).bind(until).fetch_one(&db.pool).await.unwrap();
        assert!(correct, "wrong schedule for {zone}, reset {reset}");
    }
}

#[tokio::test]
async fn expiry_preserves_unfinished_picks_even_with_lazy_cleanup_and_new_selection() {
    let db = TestDb::new().await;
    let id = task(&db, "Build <app>").await;
    let done = task(&db, "Finished").await;
    let picks = daily(&db);
    db.tasks()
        .replace_subtasks(
            id,
            &[
                Subtask::new("Design"),
                Subtask {
                    completed: true,
                    ..Subtask::new("Done")
                },
            ],
        )
        .await
        .unwrap();
    for step in [None, Some(0), Some(1)] {
        picks.select(id, step, "Asia/Kathmandu").await.unwrap();
    }
    // Existing whole-task picks from before subtask expansion still carry over.
    sqlx::query("INSERT INTO daily_tasks(task_id, subtask_id, expires_at, timezone) VALUES ($1, '', daily_reset_at('Asia/Kathmandu', statement_timestamp()), 'Asia/Kathmandu')")
        .bind(id).execute(&db.pool).await.unwrap();
    picks.select(done, None, "Asia/Kathmandu").await.unwrap();
    db.tasks().set_completed(done, true).await.unwrap();
    expire(&db).await;
    // Upsert before the sweep must capture the old whole-task selection too.
    picks.select(id, None, "Asia/Kathmandu").await.unwrap();
    let active = picks.list().await.unwrap();
    assert_eq!(active.len(), 2);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM morning_offer_items")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    let correct_schedule: bool = sqlx::query_scalar("SELECT (due_at AT TIME ZONE timezone)::time = time '08:00' AND (valid_until AT TIME ZONE timezone)::time = time '02:00' AND timezone = 'Asia/Kathmandu' FROM morning_offers")
        .fetch_one(&db.pool).await.unwrap();
    assert!(correct_schedule);
    // No delivery before local 08:00, irrespective of the process timezone.
    sqlx::query("UPDATE morning_offers SET due_at = statement_timestamp() + interval '1 hour'")
        .execute(&db.pool)
        .await
        .unwrap();
    let store = MorningStore::new(db.pool.clone());
    assert!(store.claim().await.unwrap().is_none());
    due(&db).await;
    let offer = store.claim().await.unwrap().unwrap();
    assert_eq!(offer.recipient, TELEGRAM_ID);
    assert_eq!(offer.items.len(), 2);
    assert_eq!(offer.items[0].task, "Build <app>");
    assert_eq!(offer.items[1].subtask.as_deref(), Some("Design"));
    assert!(store.claim().await.unwrap().is_none());
}

#[tokio::test]
async fn yes_revalidates_live_work_preserves_new_picks_and_is_user_scoped_and_idempotent() {
    let db = TestDb::new().await;
    let id = task(&db, "Build").await;
    let removed = task(&db, "Delete before Yes").await;
    let archived = task(&db, "Complete before Yes").await;
    let picks = daily(&db);
    db.tasks()
        .replace_subtasks(
            id,
            &[
                Subtask::new("First"),
                Subtask::new("Second"),
                Subtask::new("Removed"),
            ],
        )
        .await
        .unwrap();
    for step in [None, Some(0), Some(1), Some(2)] {
        picks.select(id, step, "America/New_York").await.unwrap();
    }
    for item in [removed, archived] {
        picks.select(item, None, "America/New_York").await.unwrap();
    }
    expire(&db).await;
    let store = MorningStore::new(db.pool.clone());
    store.sweep().await.unwrap();
    let offer = due(&db).await;
    assert_eq!(
        store.decide(offer, TELEGRAM_ID + 1, true).await.unwrap(),
        Decision::Unavailable
    );
    db.tasks().delete(removed).await.unwrap();
    db.tasks().set_completed(archived, true).await.unwrap();
    let mut steps = db.tasks().get(id).await.unwrap().unwrap().subtasks;
    steps.pop();
    steps[0].completed = true;
    steps[1].description = "Renamed".into();
    steps.swap(0, 1);
    db.tasks().replace_subtasks(id, &steps).await.unwrap();
    // A pick already added today must keep its own expiry/timezone.
    picks.select(id, Some(1), "Europe/London").await.unwrap();
    let existing = picks.list().await.unwrap()[0].expires_at;
    assert_eq!(
        store.decide(offer, TELEGRAM_ID, true).await.unwrap(),
        Decision::Accepted(1)
    );
    let active = picks.list().await.unwrap();
    assert_eq!(active.len(), 2);
    assert_eq!(
        active
            .iter()
            .find(|pick| pick.subtask_id == *steps[1].id.as_ref().unwrap())
            .unwrap()
            .expires_at,
        existing
    );
    assert_eq!(
        active
            .iter()
            .find(|pick| pick.subtask_id == *steps[0].id.as_ref().unwrap())
            .unwrap()
            .subtask_id,
        steps[0].id.as_ref().unwrap().as_str()
    );
    let expiry_correct: bool = sqlx::query_scalar("SELECT expires_at = daily_reset_at(timezone, statement_timestamp()) FROM daily_tasks WHERE subtask_id <> ''")
        .fetch_one(&db.pool).await.unwrap();
    assert!(expiry_correct);
    assert_eq!(
        store.decide(offer, TELEGRAM_ID, true).await.unwrap(),
        Decision::Unavailable
    );
    assert_eq!(
        store.decide(offer, TELEGRAM_ID, false).await.unwrap(),
        Decision::Unavailable
    );
}

#[tokio::test]
async fn no_and_expired_offers_never_add_work_and_old_backlogs_do_not_prompt() {
    let db = TestDb::new().await;
    let id = task(&db, "Build").await;
    let picks = daily(&db);
    picks.select(id, None, "UTC").await.unwrap();
    expire(&db).await;
    let store = MorningStore::new(db.pool.clone());
    store.sweep().await.unwrap();
    let offer = due(&db).await;
    assert_eq!(
        store.decide(offer, TELEGRAM_ID, false).await.unwrap(),
        Decision::Declined
    );
    assert!(picks.list().await.unwrap().is_empty());
    assert_eq!(
        store.decide(offer, TELEGRAM_ID, true).await.unwrap(),
        Decision::Unavailable
    );
    sqlx::query("UPDATE morning_offers SET state = 'sent', valid_until = statement_timestamp() - interval '1 second'").execute(&db.pool).await.unwrap();
    assert_eq!(
        store.decide(offer, TELEGRAM_ID, true).await.unwrap(),
        Decision::Unavailable
    );
    store.sweep().await.unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM morning_offers WHERE id=$1")
        .bind(offer)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(state, "expired");
    picks.select(id, None, "UTC").await.unwrap();
    sqlx::query("UPDATE daily_tasks SET expires_at = statement_timestamp() - interval '3 days'")
        .execute(&db.pool)
        .await
        .unwrap();
    store.sweep().await.unwrap();
    // Today's independent backlog reminder may exist, but old carryover must
    // never create another carryover item or restore an expired selection.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM morning_offer_items")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[derive(Default)]
struct FakeBot {
    messages: Mutex<Vec<(i64, Vec<String>)>>,
    offers: Mutex<Vec<(i64, i64, Vec<String>)>>,
    updates: Mutex<Vec<BotUpdate>>,
    offsets: Mutex<Vec<i64>>,
    answers: Mutex<Vec<String>>,
    fail: Mutex<bool>,
}
#[async_trait]
impl MessageSender for FakeBot {
    async fn send(&self, recipient: i64, messages: &[String]) -> Result<(), NotificationError> {
        self.messages
            .lock()
            .unwrap()
            .push((recipient, messages.to_vec()));
        if *self.fail.lock().unwrap() {
            Err(NotificationError::DeliveryFailed)
        } else {
            Ok(())
        }
    }
}
#[async_trait]
impl MorningBot for FakeBot {
    async fn configure_commands(&self) -> Result<(), NotificationError> {
        Ok(())
    }
    async fn offer(&self, user: i64, id: i64, text: &[String]) -> Result<(), NotificationError> {
        self.offers.lock().unwrap().push((user, id, text.to_vec()));
        if *self.fail.lock().unwrap() {
            Err(NotificationError::DeliveryFailed)
        } else {
            Ok(())
        }
    }
    async fn updates(&self, offset: i64) -> Result<Vec<BotUpdate>, NotificationError> {
        self.offsets.lock().unwrap().push(offset);
        Ok(std::mem::take(&mut *self.updates.lock().unwrap()))
    }
    async fn answer(&self, _: &str, text: &str) -> Result<(), NotificationError> {
        self.answers.lock().unwrap().push(text.into());
        Ok(())
    }
}

#[tokio::test]
async fn worker_retries_durably_and_callbacks_require_owner_private_chat_and_persist_offset() {
    let db = TestDb::new().await;
    let id = task(&db, "Build <&>").await;
    daily(&db).select(id, None, "UTC").await.unwrap();
    expire(&db).await;
    MorningStore::new(db.pool.clone()).sweep().await.unwrap();
    let offer = due(&db).await;
    let bot = Arc::new(FakeBot::default());
    *bot.fail.lock().unwrap() = true;
    let worker = MorningWorker::new(db.pool.clone(), bot.clone());
    assert!(worker.deliver_due().await.is_err());
    worker.deliver_due().await.unwrap();
    assert_eq!(bot.offers.lock().unwrap().len(), 1);
    *bot.fail.lock().unwrap() = false;
    sqlx::query("UPDATE morning_offers SET next_attempt_at = statement_timestamp()")
        .execute(&db.pool)
        .await
        .unwrap();
    // A restarted worker sees the pending delivery and retries after its lease.
    let worker = MorningWorker::new(db.pool.clone(), bot.clone());
    worker.deliver_due().await.unwrap();
    worker.deliver_due().await.unwrap();
    {
        let messages = bot.offers.lock().unwrap();
        assert_eq!(messages.len(), 2);
        assert!(messages[1].2.concat().contains("Build &lt;&amp;&gt;"));
        assert!(messages[1].2.concat().contains("Add these tasks"));
    }
    for (index, sender, chat) in [
        (1, TELEGRAM_ID + 1, TELEGRAM_ID + 1),
        (2, TELEGRAM_ID, -1),
        (3, TELEGRAM_ID, TELEGRAM_ID),
    ] {
        bot.updates.lock().unwrap().push(BotUpdate {
            id: index,
            message: None,
            callback: Some(BotCallback {
                id: index.to_string(),
                sender,
                private_chat: Some(chat),
                data: format!("morning:{offer}:yes"),
            }),
        });
        worker.poll().await.unwrap();
        assert_eq!(
            daily(&db).list().await.unwrap().len(),
            if index == 3 { 1 } else { 0 }
        );
    }
    assert_eq!(*bot.offsets.lock().unwrap(), vec![0, 2, 3]);
    MorningWorker::new(db.pool.clone(), bot.clone())
        .poll()
        .await
        .unwrap();
    assert_eq!(*bot.offsets.lock().unwrap().last().unwrap(), 4);
}

fn command(id: i64, sender: i64, private_chat: Option<i64>, text: &str) -> BotUpdate {
    BotUpdate {
        id,
        callback: None,
        message: Some(BotMessage {
            voice: None,
            sender,
            private_chat,
            text: text.into(),
        }),
    }
}

#[tokio::test]
async fn today_command_resolves_live_owned_daily_work_and_accepted_schedules_without_changing_picks()
 {
    use backend::dao::{PgCategoryDao, PgTaskDao, PgUserDao};
    let db = TestDb::new().await;
    let id = task(&db, "Build <app>").await;
    db.tasks()
        .replace_subtasks(
            id,
            &[
                Subtask {
                    completed: true,
                    ..Subtask::new("Design")
                },
                Subtask::new("Ship & verify"),
            ],
        )
        .await
        .unwrap();
    daily(&db).select(id, None, "UTC").await.unwrap();
    let expired = task(&db, "Expired pick").await;
    daily(&db).select(expired, None, "UTC").await.unwrap();
    sqlx::query("UPDATE daily_tasks SET expires_at = statement_timestamp() - interval '1 second' WHERE task_id=$1")
        .bind(expired).execute(&db.pool).await.unwrap();
    let other = PgUserDao::new(db.pool.clone())
        .get_or_create(TELEGRAM_ID + 1)
        .await
        .unwrap();
    let category = PgCategoryDao::new(db.pool.clone(), other.id)
        .create("Private category")
        .await
        .unwrap();
    let other_task = PgTaskDao::new(db.pool.clone(), other.id)
        .create(category.id, "Other user's task", TaskUrgency::Soon)
        .await
        .unwrap();
    PgDailyDao::new(db.pool.clone(), other.id)
        .select(other_task.id, None, "UTC")
        .await
        .unwrap();
    let accepted = schedule(&db, "Accepted exercise").await;
    schedule(&db, "Unaccepted exercise").await;
    backend::dao::generate_scheduled(&db.pool, None)
        .await
        .unwrap();
    sqlx::query("UPDATE scheduled_occurrences SET selected=true WHERE scheduled_task_id=$1")
        .bind(accepted)
        .execute(&db.pool)
        .await
        .unwrap();
    let picks_before = serde_json::to_value(daily(&db).list().await.unwrap()).unwrap();
    let summary_before = daily(&db).summary().await.unwrap();
    let bot = Arc::new(FakeBot::default());
    bot.updates
        .lock()
        .unwrap()
        .push(command(1, TELEGRAM_ID, Some(TELEGRAM_ID), "/today"));
    let worker = MorningWorker::new(db.pool.clone(), bot.clone());
    worker.poll().await.unwrap();
    {
        let sent = bot.messages.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, TELEGRAM_ID);
        let text = sent[0].1.concat();
        for expected in [
            "📁 Work",
            "Build &lt;app&gt;",
            "✅ Design",
            "🔹 Ship &amp; verify",
            "🔁 Scheduled",
            "Accepted exercise",
            "First step",
        ] {
            assert!(text.contains(expected), "missing {expected}");
        }
        for excluded in [
            "Expired pick",
            "Other user's task",
            "Private category",
            "Unaccepted exercise",
        ] {
            assert!(!text.contains(excluded));
        }
    }
    assert_eq!(
        serde_json::to_value(daily(&db).list().await.unwrap()).unwrap(),
        picks_before
    );
    assert_eq!(daily(&db).summary().await.unwrap(), summary_before);
    let mut steps = db.tasks().get(id).await.unwrap().unwrap().subtasks;
    steps[1].description = "Release now".into();
    db.tasks().replace_subtasks(id, &steps).await.unwrap();
    bot.updates.lock().unwrap().extend([
        command(2, TELEGRAM_ID, Some(TELEGRAM_ID), "/today"),
        command(3, other.telegram_id, Some(other.telegram_id), "/today"),
    ]);
    worker.poll().await.unwrap();
    let sent = bot.messages.lock().unwrap();
    assert!(sent[1].1.concat().contains("Release now"));
    assert!(!sent[1].1.concat().contains("Ship &amp; verify"));
    assert_eq!(sent[2].0, other.telegram_id);
    assert!(sent[2].1.concat().contains("Other user's task"));
    assert!(!sent[2].1.concat().contains("Build"));
}

#[tokio::test]
async fn commands_help_new_users_and_ignore_nonprivate_mismatched_and_unrelated_messages() {
    let db = TestDb::new().await;
    let bot = Arc::new(FakeBot::default());
    let new_user = TELEGRAM_ID + 1;
    bot.updates.lock().unwrap().extend([
        command(1, new_user, Some(new_user), "/today"),
        command(2, new_user, Some(new_user), "/start"),
        command(3, new_user, Some(new_user), "/help"),
        command(4, TELEGRAM_ID, None, "/today"),
        command(5, TELEGRAM_ID, Some(new_user), "/today"),
        command(6, TELEGRAM_ID, Some(TELEGRAM_ID), "hello"),
        command(7, TELEGRAM_ID, Some(TELEGRAM_ID), "/today_other"),
        command(8, TELEGRAM_ID, Some(TELEGRAM_ID), "/today@another_bot"),
        command(9, 0, Some(0), "/today"),
    ]);
    let worker = MorningWorker::new(db.pool.clone(), bot.clone());
    worker.poll().await.unwrap();
    assert_eq!(
        MorningStore::new(db.pool.clone()).offset().await.unwrap(),
        10
    );
    let sent = bot.messages.lock().unwrap();
    assert_eq!(sent.len(), 3);
    assert!(sent.iter().all(|(recipient, _)| *recipient == new_user));
    assert!(
        sent[0]
            .1
            .concat()
            .starts_with("No tasks in Tasks of the day yet.")
    );
    assert!(sent[1].1.concat().contains("Use /today"));
    assert_eq!(sent[1], sent[2]);
}

#[tokio::test]
async fn failed_command_delivery_does_not_replay_or_block_callbacks() {
    let db = TestDb::new().await;
    let bot = Arc::new(FakeBot::default());
    *bot.fail.lock().unwrap() = true;
    bot.updates.lock().unwrap().extend([
        command(1, TELEGRAM_ID, Some(TELEGRAM_ID), "/today"),
        BotUpdate {
            id: 2,
            message: None,
            callback: Some(BotCallback {
                id: "cb".into(),
                sender: TELEGRAM_ID,
                private_chat: Some(TELEGRAM_ID),
                data: "morning:123:yes".into(),
            }),
        },
    ]);
    MorningWorker::new(db.pool.clone(), bot.clone())
        .poll()
        .await
        .unwrap();
    assert_eq!(bot.messages.lock().unwrap().len(), 1);
    assert_eq!(bot.answers.lock().unwrap().len(), 1);
    MorningWorker::new(db.pool.clone(), bot.clone())
        .poll()
        .await
        .unwrap();
    assert_eq!(bot.messages.lock().unwrap().len(), 1);
    assert_eq!(*bot.offsets.lock().unwrap(), vec![0, 3]);
}

#[tokio::test]
async fn failed_voice_reply_advances_offset_and_does_not_block_callbacks() {
    use backend::{
        bot_voice::VoiceMessage,
        speech::{SpeechError, SpeechRecognizer},
    };
    struct Recognizer;
    #[async_trait]
    impl SpeechRecognizer for Recognizer {
        async fn transcribe(&self, _: Vec<u8>) -> Result<String, SpeechError> {
            panic!("failed download must not invoke provider");
        }
    }
    let db = TestDb::new().await;
    let bot = Arc::new(FakeBot::default());
    *bot.fail.lock().unwrap() = true;
    let mut voice = command(1, TELEGRAM_ID, Some(TELEGRAM_ID), "");
    voice.message.as_mut().unwrap().voice = Some(VoiceMessage {
        file_id: "recording".into(),
        file_size: None,
    });
    bot.updates.lock().unwrap().extend([
        voice,
        BotUpdate {
            id: 2,
            message: None,
            callback: Some(BotCallback {
                id: "after-voice".into(),
                sender: TELEGRAM_ID,
                private_chat: Some(TELEGRAM_ID),
                data: "morning:123:yes".into(),
            }),
        },
    ]);
    MorningWorker::new(db.pool.clone(), bot.clone())
        .with_speech(Arc::new(Recognizer))
        .poll()
        .await
        .unwrap();
    assert_eq!(
        MorningStore::new(db.pool.clone()).offset().await.unwrap(),
        3
    );
    assert_eq!(bot.answers.lock().unwrap().len(), 1);
    assert_eq!(bot.messages.lock().unwrap().len(), 1);
    MorningWorker::new(db.pool.clone(), bot.clone())
        .with_speech(Arc::new(Recognizer))
        .poll()
        .await
        .unwrap();
    assert_eq!(*bot.offsets.lock().unwrap(), [0, 3]);
    assert_eq!(bot.messages.lock().unwrap().len(), 1);
}

async fn schedule(db: &TestDb, description: &str) -> i64 {
    use backend::{
        dao::{PgScheduledDao, ScheduledDao},
        domain::ScheduledTaskInput,
    };
    PgScheduledDao::new(db.pool.clone(), db.user.id)
        .save(
            None,
            &ScheduledTaskInput {
                description: description.into(),
                weekdays: vec![1, 2, 3, 4, 5, 6, 7],
                timezone: "UTC".into(),
                subtasks: vec![Subtask::new("First step")],
            },
        )
        .await
        .unwrap()
        .unwrap()
        .id
}

#[tokio::test]
async fn remind_orders_all_ordinary_backlog_by_urgency_and_excludes_daily_and_other_sources() {
    use backend::dao::{PgCategoryDao, PgTaskDao, PgUserDao};
    let db = TestDb::new().await;
    let category = db.categories().create("Work <&>").await.unwrap();
    for (name, urgency) in [
        ("Later", TaskUrgency::Someday),
        ("Next", TaskUrgency::Upcoming),
        ("Urgent <&>", TaskUrgency::Soon),
        ("Urgent second", TaskUrgency::Soon),
    ] {
        let parent = db.tasks().create(category.id, name, urgency).await.unwrap();
        db.tasks()
            .replace_subtasks(parent.id, &[Subtask::new("Never include backlog subtasks")])
            .await
            .unwrap();
    }
    let picked = task(&db, "Selected parent").await;
    db.tasks()
        .replace_subtasks(picked, &[Subtask::new("One"), Subtask::new("Two")])
        .await
        .unwrap();
    daily(&db).select(picked, Some(0), "UTC").await.unwrap();
    let completed = task(&db, "Archived").await;
    db.tasks().set_completed(completed, true).await.unwrap();
    let expired = task(&db, "Expired daily pick is backlog").await;
    daily(&db).select(expired, None, "UTC").await.unwrap();
    sqlx::query("UPDATE daily_tasks SET expires_at=statement_timestamp()-interval '1 second' WHERE task_id=$1")
        .bind(expired).execute(&db.pool).await.unwrap();
    schedule(&db, "Never include recurring").await;
    sqlx::query("INSERT INTO reminder_tasks(user_id,description,due_at,timezone) VALUES ($1,'Never include reminder',statement_timestamp(),'UTC')")
        .bind(db.user.id).execute(&db.pool).await.unwrap();
    let other = PgUserDao::new(db.pool.clone())
        .get_or_create(TELEGRAM_ID + 1)
        .await
        .unwrap();
    let other_category = PgCategoryDao::new(db.pool.clone(), other.id)
        .create("Secret")
        .await
        .unwrap();
    PgTaskDao::new(db.pool.clone(), other.id)
        .create(other_category.id, "Other user", TaskUrgency::Soon)
        .await
        .unwrap();
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM daily_tasks")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    let bot = Arc::new(FakeBot::default());
    bot.updates.lock().unwrap().extend([
        command(1, TELEGRAM_ID, Some(TELEGRAM_ID), "/remind"),
        command(2, TELEGRAM_ID, None, "/remind"),
        command(3, TELEGRAM_ID, Some(other.telegram_id), "/remind"),
        command(4, TELEGRAM_ID, Some(TELEGRAM_ID), "/remind@bot"),
        command(5, TELEGRAM_ID + 2, Some(TELEGRAM_ID + 2), "/remind"),
    ]);
    MorningWorker::new(db.pool.clone(), bot.clone())
        .poll()
        .await
        .unwrap();
    let sent = bot.messages.lock().unwrap().clone();
    assert_eq!(sent.len(), 2);
    let text = sent[0].1.concat();
    for expected in ["Work &lt;&amp;&gt;", "Expired daily pick is backlog"] {
        assert!(text.contains(expected));
    }
    let positions: Vec<_> = ["Urgent &lt;&amp;&gt;", "Urgent second", "Next", "Later"]
        .map(|name| text.find(name).unwrap())
        .into();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    for excluded in [
        "Selected parent",
        "Archived",
        "Never include",
        "Other user",
        "Secret",
    ] {
        assert!(!text.contains(excluded), "unexpected {excluded}");
    }
    assert!(
        sent[1]
            .1
            .concat()
            .starts_with("No unfinished ordinary tasks")
    );
    drop(sent);
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM daily_tasks")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(before, after);
}

#[tokio::test]
async fn backlog_follows_yes_or_no_once_and_uses_post_decision_live_tasks() {
    for yes in [true, false] {
        let db = TestDb::new().await;
        let carried = task(&db, "Carry over").await;
        task(&db, "Other unfinished work").await;
        daily(&db).select(carried, None, "UTC").await.unwrap();
        expire(&db).await;
        let store = MorningStore::new(db.pool.clone());
        store.sweep().await.unwrap();
        let offer = due(&db).await;
        let bot = Arc::new(FakeBot::default());
        let worker = MorningWorker::new(db.pool.clone(), bot.clone());
        worker.deliver_due().await.unwrap();
        assert!(bot.messages.lock().unwrap().is_empty());
        bot.updates.lock().unwrap().push(BotUpdate {
            id: 1,
            message: None,
            callback: Some(BotCallback {
                id: "choice".into(),
                sender: TELEGRAM_ID,
                private_chat: Some(TELEGRAM_ID),
                data: format!("morning:{offer}:{}", if yes { "yes" } else { "no" }),
            }),
        });
        worker.poll().await.unwrap();
        // Callback replay and worker restart must not resend the follow-up.
        bot.updates.lock().unwrap().push(BotUpdate {
            id: 2,
            message: None,
            callback: Some(BotCallback {
                id: "again".into(),
                sender: TELEGRAM_ID,
                private_chat: Some(TELEGRAM_ID),
                data: format!("morning:{offer}:no"),
            }),
        });
        MorningWorker::new(db.pool.clone(), bot.clone())
            .poll()
            .await
            .unwrap();
        let messages = bot.messages.lock().unwrap();
        assert_eq!(messages.len(), 1);
        let text = messages[0].1.concat();
        assert!(text.contains("Other unfinished work"));
        assert_eq!(text.contains("Carry over"), !yes);
    }
}

#[tokio::test]
async fn backlog_without_offer_uses_local_eight_and_retries_durably_with_live_filtering() {
    let db = TestDb::new().await;
    let id = task(&db, "Unplanned work").await;
    // Save timezone history without leaving anything eligible for carryover.
    sqlx::query("INSERT INTO morning_offers(user_id,timezone,reset_at,due_at,valid_until,state) VALUES ($1,'Asia/Kathmandu','2026-01-01Z','2026-01-01Z','2026-01-02Z','expired')")
        .bind(db.user.id).execute(&db.pool).await.unwrap();
    for instant in [
        "2026-03-29T02:14:59Z",
        "2026-03-29T02:15:00Z",
        "2026-03-29T03:00:00Z",
    ] {
        sqlx::query("SELECT prepare_backlog_morning_offers($1::text::timestamptz)")
            .bind(instant)
            .execute(&db.pool)
            .await
            .unwrap();
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM morning_offers WHERE state='pending'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(count, if instant.ends_with("14:59Z") { 0 } else { 1 });
    }
    let valid_schedule: bool = sqlx::query_scalar("SELECT timezone='Asia/Kathmandu' AND due_at='2026-03-29T02:15Z' AND valid_until='2026-03-29T20:15Z' FROM morning_offers WHERE state='pending'")
        .fetch_one(&db.pool).await.unwrap();
    assert!(valid_schedule);
    sqlx::query("DELETE FROM morning_offers WHERE state='expired'")
        .execute(&db.pool)
        .await
        .unwrap();
    let offer = due(&db).await;
    let bot = Arc::new(FakeBot::default());
    *bot.fail.lock().unwrap() = true;
    let worker = MorningWorker::new(db.pool.clone(), bot.clone());
    assert!(worker.deliver_due().await.is_err());
    assert!(bot.offers.lock().unwrap().is_empty());
    assert_eq!(bot.messages.lock().unwrap().len(), 1);
    worker.deliver_due().await.unwrap();
    assert_eq!(bot.messages.lock().unwrap().len(), 1);
    *bot.fail.lock().unwrap() = false;
    // Work added to today while delivery failed must disappear on retry.
    daily(&db).select(id, None, "Asia/Kathmandu").await.unwrap();
    sqlx::query(
        "UPDATE morning_offers SET backlog_next_attempt_at=statement_timestamp() WHERE id=$1",
    )
    .bind(offer)
    .execute(&db.pool)
    .await
    .unwrap();
    MorningWorker::new(db.pool.clone(), bot.clone())
        .deliver_due()
        .await
        .unwrap();
    assert_eq!(bot.messages.lock().unwrap().len(), 1);
    let sent: bool = sqlx::query_scalar("SELECT backlog_sent FROM morning_offers WHERE id=$1")
        .bind(offer)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(sent);
}

#[tokio::test]
async fn multiple_due_timezone_groups_wait_for_all_choices_and_share_one_backlog() {
    let db = TestDb::new().await;
    task(&db, "Unplanned").await;
    let store = MorningStore::new(db.pool.clone());
    let mut offers = Vec::new();
    for zone in ["UTC", "Etc/UTC"] {
        let id: i64 = sqlx::query_scalar("INSERT INTO morning_offers(user_id,timezone,reset_at,due_at,valid_until) VALUES ($1,$2,statement_timestamp()-interval '8 hours',statement_timestamp()-interval '1 minute',statement_timestamp()+interval '1 hour') RETURNING id")
            .bind(db.user.id).bind(zone).fetch_one(&db.pool).await.unwrap();
        offers.push(id);
    }
    assert_eq!(
        store.decide(offers[0], TELEGRAM_ID, false).await.unwrap(),
        Decision::Declined
    );
    assert!(store.claim_backlog().await.unwrap().is_none());
    assert_eq!(
        store.decide(offers[1], TELEGRAM_ID, true).await.unwrap(),
        Decision::Accepted(0)
    );
    let claimed = store.claim_backlog().await.unwrap().unwrap();
    assert_eq!(claimed.1, db.user.id);
    assert_eq!(claimed.2, TELEGRAM_ID);
    assert!(store.claim_backlog().await.unwrap().is_none());
    store.backlog_sent(claimed.0).await.unwrap();
    assert!(store.claim_backlog().await.unwrap().is_none());
}

#[tokio::test]
async fn backlog_morning_defaults_to_utc_and_uses_dst_calendar_dates_without_backfill() {
    let db = TestDb::new().await;
    task(&db, "Plan").await;
    sqlx::query("SELECT prepare_backlog_morning_offers('2026-03-08T07:59:59Z')")
        .execute(&db.pool)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM morning_offers")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::query("SELECT prepare_backlog_morning_offers('2026-03-08T08:00:00Z')")
        .execute(&db.pool)
        .await
        .unwrap();
    let zone: String = sqlx::query_scalar("SELECT timezone FROM morning_offers")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(zone, "UTC");
    sqlx::query("UPDATE morning_offers SET timezone='America/New_York', state='expired'")
        .execute(&db.pool)
        .await
        .unwrap();
    // Fall-back day: local 08:00 is 13:00 UTC, not yesterday's UTC offset.
    sqlx::query("SELECT prepare_backlog_morning_offers('2026-11-01T12:59:59Z')")
        .execute(&db.pool)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM morning_offers")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    sqlx::query("SELECT prepare_backlog_morning_offers('2026-11-01T13:00:00Z')")
        .execute(&db.pool)
        .await
        .unwrap();
    let valid: bool = sqlx::query_scalar("SELECT timezone='America/New_York' AND due_at='2026-11-01T13:00Z' AND valid_until='2026-11-02T07:00Z' FROM morning_offers WHERE state='pending'").fetch_one(&db.pool).await.unwrap();
    assert!(valid);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM morning_offers")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 2); // No groups for the months of downtime.
}

#[tokio::test]
async fn scheduled_only_mornings_offer_without_selecting_and_yes_selects_once() {
    use backend::dao::{PgScheduledDao, ScheduledDao};
    let db = TestDb::new().await;
    let template = schedule(&db, "Exercise <&>").await;
    let scheduled = PgScheduledDao::new(db.pool.clone(), db.user.id);
    assert!(scheduled.daily().await.unwrap().is_empty());
    assert!(daily(&db).summary().await.unwrap().is_empty());
    let occurrence: i64 = sqlx::query_scalar("SELECT id FROM scheduled_occurrences")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(
        !scheduled
            .progress(occurrence, Some(true), None)
            .await
            .unwrap()
    );
    assert!(!scheduled.dismiss(occurrence).await.unwrap());

    let store = MorningStore::new(db.pool.clone());
    store.sweep().await.unwrap();
    let correct: bool = sqlx::query_scalar("SELECT (due_at AT TIME ZONE timezone)::time = time '08:00' AND (valid_until AT TIME ZONE timezone)::time = time '02:00' FROM morning_offers")
        .fetch_one(&db.pool).await.unwrap();
    assert!(correct);
    sqlx::query("UPDATE morning_offers SET due_at = statement_timestamp() + interval '1 hour'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(store.claim().await.unwrap().is_none());
    let id = due(&db).await;
    let bot = Arc::new(FakeBot::default());
    let worker = MorningWorker::new(db.pool.clone(), bot.clone());
    worker.deliver_due().await.unwrap();
    worker.deliver_due().await.unwrap();
    {
        let messages = bot.offers.lock().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0, TELEGRAM_ID);
        assert!(messages[0].2.concat().contains("Exercise &lt;&amp;&gt;"));
    }
    assert!(scheduled.daily().await.unwrap().is_empty());
    assert_eq!(
        store.decide(id, TELEGRAM_ID + 1, true).await.unwrap(),
        Decision::Unavailable
    );
    assert_eq!(
        store.decide(id, TELEGRAM_ID, true).await.unwrap(),
        Decision::Accepted(1)
    );
    let picks = scheduled.daily().await.unwrap();
    assert_eq!(picks.len(), 1);
    assert_eq!(picks[0].scheduled_task_id, template);
    assert!(!picks[0].completed);
    assert!(!picks[0].subtasks[0].completed);
    assert_eq!(daily(&db).summary().await.unwrap().len(), 1);
    assert_eq!(
        store.decide(id, TELEGRAM_ID, true).await.unwrap(),
        Decision::Unavailable
    );
    scheduled.dismiss(occurrence).await.unwrap();
    worker.deliver_due().await.unwrap();
    assert!(scheduled.daily().await.unwrap().is_empty());
    assert_eq!(bot.offers.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn scheduled_and_carryover_share_offer_and_no_keeps_both_out_of_daily_list() {
    use backend::dao::{PgScheduledDao, ScheduledDao};
    let db = TestDb::new().await;
    let ordinary = task(&db, "Carry me").await;
    daily(&db).select(ordinary, None, "UTC").await.unwrap();
    // Expire at today's real reset so ordinary and scheduled work share a group.
    sqlx::query("UPDATE daily_tasks SET expires_at = ((daily_reset_at('UTC', statement_timestamp()) AT TIME ZONE 'UTC')::date - 1 + time '02:00') AT TIME ZONE 'UTC'")
        .execute(&db.pool).await.unwrap();
    schedule(&db, "Scheduled").await;
    backend::dao::generate_scheduled(&db.pool, None)
        .await
        .unwrap();
    let store = MorningStore::new(db.pool.clone());
    store.sweep().await.unwrap();
    let id = due(&db).await;
    let offer = store.claim().await.unwrap().unwrap();
    assert_eq!(offer.items.len(), 2);
    assert!(offer.items.iter().any(|i| i.task == "Carry me"));
    assert!(offer.items.iter().any(|i| i.task == "Scheduled"));
    assert_ne!(offer.items[0].task_id, offer.items[1].task_id);
    assert_eq!(
        store.decide(id, TELEGRAM_ID, false).await.unwrap(),
        Decision::Declined
    );
    store.sweep().await.unwrap();
    assert!(store.claim().await.unwrap().is_none());
    assert!(daily(&db).list().await.unwrap().is_empty());
    assert!(
        PgScheduledDao::new(db.pool.clone(), db.user.id)
            .daily()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn scheduled_acceptance_filters_deleted_hidden_expired_and_unoffered_work() {
    use backend::dao::{PgScheduledDao, ScheduledDao};
    let db = TestDb::new().await;
    for name in ["Keep", "Deleted", "Hidden", "Expired"] {
        schedule(&db, name).await;
    }
    backend::dao::generate_scheduled(&db.pool, None)
        .await
        .unwrap();
    let store = MorningStore::new(db.pool.clone());
    store.sweep().await.unwrap();
    let id = due(&db).await;
    let offer = store.claim().await.unwrap().unwrap();
    assert_eq!(offer.items.len(), 4);
    store.sent(id).await.unwrap();
    sqlx::query("DELETE FROM scheduled_tasks WHERE description = 'Deleted'")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE scheduled_occurrences SET hidden = true WHERE description = 'Hidden'")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE scheduled_occurrences SET expires_at = statement_timestamp() - interval '1 second' WHERE description = 'Expired'")
        .execute(&db.pool).await.unwrap();
    schedule(&db, "Created after offer").await;
    backend::dao::generate_scheduled(&db.pool, None)
        .await
        .unwrap();
    store.sweep().await.unwrap();
    assert_eq!(
        store.decide(id, TELEGRAM_ID, true).await.unwrap(),
        Decision::Accepted(1)
    );
    let items = PgScheduledDao::new(db.pool.clone(), db.user.id)
        .daily()
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].description, "Keep");
}

#[tokio::test]
async fn scheduled_offers_follow_due_weekdays_dst_and_snapshot_timezone_without_backfill() {
    let db = TestDb::new().await;
    let id = schedule(&db, "Sunday exercise").await;
    sqlx::query("UPDATE scheduled_tasks SET weekdays=ARRAY[7]::smallint[], timezone='Europe/Berlin', created_at='2026-01-01Z' WHERE id=$1")
        .bind(id).execute(&db.pool).await.unwrap();
    // Missing local 02:00 becomes 03:00 on spring-forward Sunday.
    for instant in ["2026-03-29T00:59:00Z", "2026-03-29T01:00:00Z"] {
        sqlx::query("SELECT generate_scheduled_occurrences($1, $2::text::timestamptz)")
            .bind(db.user.id)
            .bind(instant)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("SELECT prepare_scheduled_morning_offers($1::text::timestamptz)")
            .bind(instant)
            .execute(&db.pool)
            .await
            .unwrap();
    }
    let schedule: (String, String, String) = sqlx::query_as("SELECT to_char(reset_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI'), to_char(due_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI'), to_char(valid_until AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI') FROM morning_offers")
        .fetch_one(&db.pool).await.unwrap();
    assert_eq!(
        schedule,
        (
            "2026-03-29 01:00".into(),
            "2026-03-29 06:00".into(),
            "2026-03-30 00:00".into()
        )
    );
    sqlx::query("UPDATE scheduled_tasks SET timezone='Asia/Kathmandu'")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("SELECT prepare_scheduled_morning_offers('2026-03-29T07:00:00Z')")
        .execute(&db.pool)
        .await
        .unwrap();
    let zones: Vec<String> = sqlx::query_scalar("SELECT timezone FROM morning_offers")
        .fetch_all(&db.pool)
        .await
        .unwrap();
    assert_eq!(zones, vec!["Europe/Berlin"]);
    // Monday is not due. Expired Sunday work must not produce another offer.
    sqlx::query("SELECT generate_scheduled_occurrences($1, '2026-03-30T12:00:00Z')")
        .bind(db.user.id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("SELECT prepare_scheduled_morning_offers('2026-03-30T12:00:00Z')")
        .execute(&db.pool)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM morning_offers")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
