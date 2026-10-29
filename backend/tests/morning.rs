mod support;

use async_trait::async_trait;
use backend::{
    dao::{
        CategoryDao, DailyDao, PgDailyDao, TaskDao,
        morning::{Decision, MorningStore},
    },
    domain::{Subtask, TaskUrgency},
    morning::{BotCallback, BotUpdate, MorningBot, MorningWorker},
    service::NotificationError,
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
    picks.select(done, None, "Asia/Kathmandu").await.unwrap();
    db.tasks().set_completed(done, true).await.unwrap();
    expire(&db).await;
    // Upsert before the sweep must capture the old whole-task selection too.
    picks.select(id, None, "Asia/Kathmandu").await.unwrap();
    let active = picks.list().await.unwrap();
    assert_eq!(active.len(), 1);
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
    picks.select(id, None, "Europe/London").await.unwrap();
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
            .find(|pick| pick.subtask_id.is_empty())
            .unwrap()
            .expires_at,
        existing
    );
    assert_eq!(
        active
            .iter()
            .find(|pick| !pick.subtask_id.is_empty())
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
    let state: String = sqlx::query_scalar("SELECT state FROM morning_offers")
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
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM morning_offers")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[derive(Default)]
struct FakeBot {
    offers: Mutex<Vec<(i64, i64, Vec<String>)>>,
    updates: Mutex<Vec<BotUpdate>>,
    offsets: Mutex<Vec<i64>>,
    answers: Mutex<Vec<String>>,
    fail: Mutex<bool>,
}
#[async_trait]
impl MorningBot for FakeBot {
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
        assert!(messages[1].2.concat().contains("Add this unfinished work"));
    }
    for (index, sender, chat) in [
        (1, TELEGRAM_ID + 1, TELEGRAM_ID + 1),
        (2, TELEGRAM_ID, -1),
        (3, TELEGRAM_ID, TELEGRAM_ID),
    ] {
        bot.updates.lock().unwrap().push(BotUpdate {
            id: index,
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
