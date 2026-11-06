mod support;

use backend::{
    dao::{
        CategoryDao, DailyDao, PgDailyDao, TaskDao, maintenance,
        morning::{Decision, MorningStore},
    },
    domain::TaskUrgency,
};
use support::{TELEGRAM_ID, TestDb};

#[tokio::test]
async fn upgrade_removes_application_routines_and_triggers_without_changing_saved_work() {
    let db = TestDb::new_at_version(17).await;
    let category = db.categories().create("Work").await.unwrap();
    let task = db
        .tasks()
        .create(category.id, "Keep progress", TaskUrgency::Soon)
        .await
        .unwrap();
    let daily = PgDailyDao::new(db.pool.clone(), db.user.id);
    daily.select(task.id, None, "Europe/Berlin").await.unwrap();
    let picks = daily.list().await.unwrap();
    sqlx::query("INSERT INTO reminder_tasks(user_id,description,due_at,timezone,completed) VALUES ($1,'Saved reminder',statement_timestamp(),'UTC',true)")
        .bind(db.user.id).execute(&db.pool).await.unwrap();
    backend::db::MIGRATOR.run(&db.pool).await.unwrap();
    backend::db::MIGRATOR.run(&db.pool).await.unwrap();
    assert_eq!(
        serde_json::to_value(daily.list().await.unwrap()).unwrap(),
        serde_json::to_value(picks).unwrap()
    );
    assert_eq!(db.tasks().get(task.id).await.unwrap().unwrap(), task);
    let routines: i64=sqlx::query_scalar("SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='public'").fetch_one(&db.pool).await.unwrap();
    let triggers: i64=sqlx::query_scalar("SELECT count(*) FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND NOT t.tgisinternal").fetch_one(&db.pool).await.unwrap();
    assert_eq!(routines, 0);
    assert_eq!(triggers, 0);
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT completed FROM reminder_tasks")
            .fetch_one(&db.pool)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn rollover_is_atomic_with_cleanup_and_concurrent_selection() {
    let db = TestDb::new().await;
    let category = db.categories().create("Work").await.unwrap();
    let task = db
        .tasks()
        .create(category.id, "Carry", TaskUrgency::Soon)
        .await
        .unwrap();
    let daily = PgDailyDao::new(db.pool.clone(), db.user.id);
    daily.select(task.id, None, "UTC").await.unwrap();
    sqlx::query("UPDATE daily_tasks SET expires_at=statement_timestamp()-interval '1 minute'")
        .execute(&db.pool)
        .await
        .unwrap();
    let mut tx = db.pool.begin().await.unwrap();
    let instant = maintenance::now(&mut tx).await.unwrap();
    maintenance::expire_picks(&mut tx, "daily_tasks", Some(db.user.id), None, instant)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM daily_tasks")
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM morning_offer_items")
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        1
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM daily_tasks")
            .fetch_one(&db.pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM morning_offers")
            .fetch_one(&db.pool)
            .await
            .unwrap(),
        0
    );
    let store = MorningStore::new(db.pool.clone());
    let (a, b, c) = tokio::join!(
        store.sweep(),
        daily.select(task.id, None, "UTC"),
        daily.list()
    );
    a.unwrap();
    b.unwrap();
    c.unwrap();
    assert_eq!(daily.list().await.unwrap().len(), 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM morning_offer_items")
            .fetch_one(&db.pool)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn reminder_acceptance_caps_expiry_even_when_offer_was_extended() {
    let db = TestDb::new().await;
    let instant = support::clock(&db).await;
    let day = instant.date_naive();
    let event = backend::planning::at(chrono_tz::UTC, day, 15);
    let expiry = backend::planning::at(chrono_tz::UTC, day.succ_opt().unwrap(), 2);
    let reminder: i64=sqlx::query_scalar("INSERT INTO reminder_tasks(user_id,description,due_at,timezone) VALUES ($1,'Event',$2,'UTC') RETURNING id")
        .bind(db.user.id).bind(event).fetch_one(&db.pool).await.unwrap();
    let offer: i64=sqlx::query_scalar("INSERT INTO morning_offers(user_id,timezone,reset_at,due_at,valid_until) VALUES ($1,'UTC',$2,$2,$3) RETURNING id")
        .bind(db.user.id).bind(instant-chrono::Duration::minutes(1)).bind(expiry+chrono::Duration::days(1)).fetch_one(&db.pool).await.unwrap();
    sqlx::query("INSERT INTO morning_offer_reminder_items(offer_id,reminder_id,due_at,timezone,kind) VALUES ($1,$2,$3,'UTC','today')")
        .bind(offer).bind(reminder).bind(event).execute(&db.pool).await.unwrap();
    assert_eq!(
        MorningStore::new(db.pool.clone())
            .decide(offer, TELEGRAM_ID, true)
            .await
            .unwrap(),
        Decision::Accepted(1)
    );
    let selected: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT selected_until FROM reminder_tasks WHERE id=$1")
            .bind(reminder)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(selected, expiry);
}

#[tokio::test]
async fn rust_calendar_resolution_matches_postgres_for_iana_transitions() {
    let db = TestDb::new().await;
    for (zone, date, hour) in [
        ("Europe/Berlin", "2026-03-29", 2),
        ("Europe/Berlin", "2026-10-25", 2),
        ("America/New_York", "2026-03-08", 2),
        ("America/New_York", "2026-11-01", 1),
        ("Australia/Lord_Howe", "2026-10-04", 2),
        ("Pacific/Apia", "2011-12-30", 2),
        ("Asia/Kathmandu", "2026-10-29", 2),
    ] {
        let expected: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
            "SELECT ($1::text::date + make_interval(hours => $2)) AT TIME ZONE $3",
        )
        .bind(date)
        .bind(hour as i32)
        .bind(zone)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        let day = date.parse().unwrap();
        assert_eq!(
            backend::planning::at(backend::planning::zone(zone).unwrap(), day, hour),
            expected,
            "{zone} {date}"
        );
    }
}
