//! Explicit application-owned replacements for the former database triggers.
//! Call within the same transaction as any deletion/replacement of daily picks.
use crate::{domain::Subtask, planning};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use sqlx::{PgConnection, PgPool, types::Json};

type ReminderCandidate = (i64, i64, String, DateTime<Utc>, Option<DateTime<Utc>>);

pub(super) fn timezone(name: &str) -> Result<chrono_tz::Tz, sqlx::Error> {
    planning::zone(name)
        .map_err(|_| sqlx::Error::Protocol(format!("invalid saved timezone: {name}")))
}

pub async fn now(connection: &mut PgConnection) -> Result<DateTime<Utc>, sqlx::Error> {
    sqlx::query_scalar("SELECT statement_timestamp()")
        .fetch_one(connection)
        .await
}

/// Delete completed reminders at 02:00 after their saved-zone event date.
/// Lock rows so reopening or rescheduling cannot race with the cleanup decision.
pub async fn expire_completed_reminders(
    connection: &mut PgConnection,
    owner: Option<i64>,
    instant: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let rows: Vec<(i64, DateTime<Utc>, String)> = sqlx::query_as(
        "SELECT id,due_at,timezone FROM reminder_tasks
         WHERE completed AND ($1::bigint IS NULL OR user_id=$1) AND due_at <= $2
         ORDER BY id FOR UPDATE",
    )
    .bind(owner)
    .bind(instant)
    .fetch_all(&mut *connection)
    .await?;
    for (id, event, timezone) in rows {
        let zone = self::timezone(&timezone)?;
        let next_day = event.with_timezone(&zone).date_naive().succ_opt().unwrap();
        if planning::at(zone, next_day, 2) <= instant {
            sqlx::query("DELETE FROM reminder_tasks WHERE id=$1")
                .bind(id)
                .execute(&mut *connection)
                .await?;
        }
    }
    Ok(())
}

async fn offer(
    connection: &mut PgConnection,
    owner: i64,
    timezone: &str,
    reset: DateTime<Utc>,
    due: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<i64, sqlx::Error> {
    sqlx::query("INSERT INTO morning_offers(user_id,timezone,reset_at,due_at,valid_until) VALUES ($1,$2,$3,$4,$5) ON CONFLICT(user_id,timezone,reset_at) DO NOTHING")
        .bind(owner).bind(timezone).bind(reset).bind(due).bind(until).execute(&mut *connection).await?;
    sqlx::query_scalar(
        "SELECT id FROM morning_offers WHERE user_id=$1 AND timezone=$2 AND reset_at=$3",
    )
    .bind(owner)
    .bind(timezone)
    .bind(reset)
    .fetch_one(connection)
    .await
}

/// Preserve then delete expired rows atomically, including lazy reads, explicit
/// removal and selection upserts. Scope to a user/task when invoked by API work.
pub async fn expire_picks(
    connection: &mut PgConnection,
    table: &str,
    owner: Option<i64>,
    task: Option<i64>,
    instant: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    assert!(matches!(table, "daily_tasks" | "tomorrow_tasks"));
    let tasks: Vec<(i64, i64, bool, Json<Vec<Subtask>>)> = sqlx::query_as(&format!(
        "SELECT t.id,c.user_id,t.completed,t.subtasks FROM tasks t JOIN categories c ON c.id=t.category_id
         WHERE ($1::bigint IS NULL OR c.user_id=$1) AND ($2::bigint IS NULL OR t.id=$2)
         AND EXISTS(SELECT 1 FROM {table} d WHERE d.task_id=t.id AND d.expires_at <= $3)
         ORDER BY t.id FOR UPDATE OF t"))
        .bind(owner).bind(task).bind(instant).fetch_all(&mut *connection).await?;
    for (task_id, user_id, completed, Json(steps)) in tasks {
        let picks: Vec<(String, DateTime<Utc>, String)> = sqlx::query_as(&format!(
            "SELECT subtask_id,expires_at,timezone FROM {table} WHERE task_id=$1 AND expires_at <= $2 ORDER BY subtask_id FOR UPDATE"))
            .bind(task_id).bind(instant).fetch_all(&mut *connection).await?;
        for (step, reset, timezone) in picks {
            let (due, until) = planning::morning_schedule(self::timezone(&timezone)?, reset);
            if !completed
                && until > instant
                && (step.is_empty()
                    || steps
                        .iter()
                        .any(|s| s.id.as_deref() == Some(&step) && !s.completed))
            {
                let id = offer(connection, user_id, &timezone, reset, due, until).await?;
                sqlx::query("INSERT INTO morning_offer_items(offer_id,task_id,subtask_id) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
                    .bind(id).bind(task_id).bind(&step).execute(&mut *connection).await?;
            }
        }
        sqlx::query(&format!(
            "DELETE FROM {table} WHERE task_id=$1 AND expires_at <= $2"
        ))
        .bind(task_id)
        .bind(instant)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

/// Generate current-day snapshots only. Template row locks serialize generation
/// with template edits/deletion; uniqueness makes concurrent callers idempotent.
pub async fn generate_scheduled_at(
    pool: &PgPool,
    owner: Option<i64>,
    instant: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    #[derive(sqlx::FromRow)]
    struct Template {
        id: i64,
        description: String,
        detailed_description: String,
        timezone: String,
        weekdays: Vec<i16>,
        subtasks: Json<Vec<Subtask>>,
    }
    let templates = sqlx::query_as::<_, Template>("SELECT id,description,detailed_description,timezone,weekdays,subtasks FROM scheduled_tasks WHERE ($1::bigint IS NULL OR user_id=$1) AND created_at <= $2 ORDER BY id FOR SHARE")
        .bind(owner).bind(instant).fetch_all(&mut *tx).await?;
    for mut template in templates {
        let zone = timezone(&template.timezone)?;
        let day = planning::planning_date(zone, instant);
        if !template
            .weekdays
            .contains(&(day.weekday().number_from_monday() as i16))
        {
            continue;
        }
        for step in &mut template.subtasks.0 {
            step.id = None;
            step.completed = false;
        }
        sqlx::query("INSERT INTO scheduled_occurrences(scheduled_task_id,planning_date,description,detailed_description,subtasks,expires_at,timezone) VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(scheduled_task_id,planning_date) DO NOTHING")
            .bind(template.id).bind(day).bind(template.description).bind(template.detailed_description).bind(template.subtasks).bind(planning::daily_reset(zone,instant)).bind(template.timezone).execute(&mut *tx).await?;
    }
    tx.commit().await
}

pub async fn prepare_scheduled_at(
    pool: &PgPool,
    instant: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let rows: Vec<(i64,i64,String,NaiveDate,DateTime<Utc>)> = sqlx::query_as("SELECT o.id,s.user_id,o.timezone,o.planning_date,o.expires_at FROM scheduled_occurrences o JOIN scheduled_tasks s ON s.id=o.scheduled_task_id WHERE NOT o.selected AND NOT o.hidden AND NOT o.completed AND o.expires_at > $1 ORDER BY o.id")
        .bind(instant).fetch_all(pool).await?;
    let mut groups = std::collections::BTreeMap::new();
    for (occurrence, owner, timezone, day, until) in rows {
        let group = groups
            .entry((owner, timezone, day))
            .or_insert_with(|| (until, Vec::new()));
        group.1.push(occurrence);
    }
    // One group per transaction: claim/decision sees its complete snapshot, and
    // preparation never holds one offer lock while waiting for another offer.
    for ((owner, timezone, day), (until, occurrences)) in groups {
        let mut tx = pool.begin().await?;
        let zone = self::timezone(&timezone)?;
        let id = offer(
            &mut tx,
            owner,
            &timezone,
            planning::at(zone, day, 2),
            planning::at(zone, day, 8),
            until,
        )
        .await?;
        sqlx::query("SELECT id FROM morning_offers WHERE id=$1 FOR NO KEY UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO morning_offer_scheduled_items(offer_id,occurrence_id) SELECT m.id,o.id FROM morning_offers m,scheduled_occurrences o JOIN scheduled_tasks s ON s.id=o.scheduled_task_id WHERE m.id=$1 AND m.state='pending' AND o.id=ANY($2) AND s.user_id=m.user_id AND NOT o.selected AND NOT o.hidden AND NOT o.completed AND o.expires_at > $3 ON CONFLICT DO NOTHING")
            .bind(id).bind(occurrences).bind(instant).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    Ok(())
}

pub async fn prepare_reminders_at(
    pool: &PgPool,
    instant: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let rows: Vec<ReminderCandidate> = sqlx::query_as("SELECT id,user_id,timezone,due_at,selected_until FROM reminder_tasks WHERE NOT completed AND due_at >= $1 AND due_at <= $2 ORDER BY id")
        .bind(instant-Duration::days(3)).bind(instant+Duration::days(3)).fetch_all(pool).await?;
    let mut groups = std::collections::BTreeMap::new();
    for (reminder, owner, timezone, event, selected) in rows {
        let zone = self::timezone(&timezone)?;
        for (offset, kind) in [(-1, "yesterday"), (0, "today"), (1, "tomorrow")] {
            if offset != -1 && selected.is_some() {
                continue;
            }
            let day = event.with_timezone(&zone).date_naive() - Duration::days(offset);
            let reset = planning::at(zone, day, 2);
            let until = planning::at(zone, day.succ_opt().unwrap(), 2);
            if reset > instant || until <= instant {
                continue;
            }
            groups
                .entry((owner, timezone.clone(), day))
                .or_insert_with(Vec::new)
                .push((reminder, event, kind));
        }
    }
    for ((owner, timezone, day), reminders) in groups {
        let mut tx = pool.begin().await?;
        let zone = self::timezone(&timezone)?;
        let id = offer(
            &mut tx,
            owner,
            &timezone,
            planning::at(zone, day, 2),
            planning::at(zone, day, 8),
            planning::at(zone, day.succ_opt().unwrap(), 2),
        )
        .await?;
        sqlx::query("SELECT id FROM morning_offers WHERE id=$1 FOR NO KEY UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        for (reminder, event, kind) in reminders {
            sqlx::query("INSERT INTO morning_offer_reminder_items(offer_id,reminder_id,due_at,timezone,kind) SELECT m.id,r.id,r.due_at,r.timezone,$5 FROM morning_offers m,reminder_tasks r WHERE m.id=$1 AND m.state='pending' AND m.valid_until > $6 AND r.id=$2 AND r.user_id=m.user_id AND r.due_at=$3 AND r.timezone=$4 AND NOT r.completed AND ($5='yesterday' OR r.selected_until IS NULL) ON CONFLICT DO NOTHING")
                    .bind(id).bind(reminder).bind(event).bind(&timezone).bind(kind).bind(instant).execute(&mut *tx).await?;
        }
        tx.commit().await?;
    }
    Ok(())
}

pub async fn prepare_backlog_at(pool: &PgPool, instant: DateTime<Utc>) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let owners: Vec<(i64,String)>=sqlx::query_as("SELECT u.id,COALESCE(
        (SELECT p.timezone FROM daily_tasks p JOIN tasks t ON t.id=p.task_id JOIN categories c ON c.id=t.category_id WHERE c.user_id=u.id ORDER BY p.expires_at DESC,t.id DESC LIMIT 1),
        (SELECT o.timezone FROM morning_offers o WHERE o.user_id=u.id ORDER BY o.reset_at DESC,o.id DESC LIMIT 1),
        (SELECT timezone FROM scheduled_tasks WHERE user_id=u.id ORDER BY id DESC LIMIT 1),
        (SELECT timezone FROM reminder_tasks WHERE user_id=u.id ORDER BY id DESC LIMIT 1),'UTC')
        FROM users u WHERE EXISTS(SELECT 1 FROM tasks t JOIN categories c ON c.id=t.category_id WHERE c.user_id=u.id AND NOT t.completed)
        AND NOT EXISTS(SELECT 1 FROM morning_offers o WHERE o.user_id=u.id AND o.reset_at <= $1 AND o.valid_until > $1) ORDER BY u.id")
        .bind(instant).fetch_all(&mut *tx).await?;
    for (owner, timezone) in owners {
        let zone = self::timezone(&timezone)?;
        let day = planning::planning_date(zone, instant);
        let due = planning::at(zone, day, 8);
        let until = planning::at(zone, day.succ_opt().unwrap(), 2);
        if due <= instant && until > instant {
            offer(
                &mut tx,
                owner,
                &timezone,
                planning::at(zone, day, 2),
                due,
                until,
            )
            .await?;
        }
    }
    tx.commit().await
}
