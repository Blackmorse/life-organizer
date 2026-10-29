//! Durable morning-offer orchestration. Telegram transport and SQL stay behind
//! their own boundaries; callbacks never trust a client-supplied owner.
use crate::{
    dao::morning::{Decision, MorningStore},
    service::{NotificationError, summary_messages},
};
use async_trait::async_trait;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};

pub struct BotUpdate {
    pub id: i64,
    pub callback: Option<BotCallback>,
}

pub struct BotCallback {
    pub id: String,
    pub sender: i64,
    pub private_chat: Option<i64>,
    pub data: String,
}

#[async_trait]
pub trait MorningBot: Send + Sync {
    async fn offer(
        &self,
        recipient: i64,
        offer_id: i64,
        messages: &[String],
    ) -> Result<(), NotificationError>;
    async fn updates(&self, offset: i64) -> Result<Vec<BotUpdate>, NotificationError>;
    async fn answer(&self, callback_id: &str, text: &str) -> Result<(), NotificationError>;
}

pub struct MorningWorker {
    pool: PgPool,
    store: MorningStore,
    bot: Arc<dyn MorningBot>,
}

impl MorningWorker {
    pub fn new(pool: PgPool, bot: Arc<dyn MorningBot>) -> Self {
        Self {
            pool: pool.clone(),
            store: MorningStore::new(pool),
            bot,
        }
    }

    pub async fn deliver_due(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        crate::dao::generate_scheduled(&self.pool, None).await?;
        self.store.sweep().await?;
        // Bound each batch so callbacks remain responsive even with a backlog.
        for _ in 0..20 {
            let Some(offer) = self.store.claim().await? else {
                break;
            };
            if !offer.items.is_empty() {
                let messages = summary_messages(
                    &offer.items,
                    "Unfinished tasks from your previous daily list",
                    "Add this unfinished work to Tasks of the day?",
                );
                self.bot.offer(offer.recipient, offer.id, &messages).await?;
            }
            self.store.sent(offer.id).await?;
        }
        Ok(())
    }

    pub async fn poll(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for update in self.bot.updates(self.store.offset().await?).await? {
            if let Some(callback) = update.callback {
                let text = if let Some((id, yes)) = parse_choice(&callback.data)
                    .filter(|_| callback.private_chat == Some(callback.sender))
                {
                    match self.store.decide(id, callback.sender, yes).await? {
                        Decision::Accepted(_) => {
                            "Added unfinished work to Tasks of the day. Open or refresh the Mini App to see it."
                        }
                        Decision::Declined => "No tasks added.",
                        Decision::Unavailable => "This offer has expired or was already answered.",
                    }
                } else {
                    "This button is unavailable."
                };
                // The choice is already durable/idempotent. A failed callback
                // acknowledgement must not block later updates indefinitely.
                let _ = self.bot.answer(&callback.id, text).await;
            }
            self.store.advance(update.id + 1).await?;
        }
        Ok(())
    }
}

fn parse_choice(data: &str) -> Option<(i64, bool)> {
    let mut parts = data.split(':');
    if parts.next()? != "morning" {
        return None;
    }
    let id = parts.next()?.parse::<i64>().ok().filter(|id| *id > 0)?;
    let yes = match parts.next()? {
        "yes" => true,
        "no" => false,
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((id, yes))
}

/// One active poller per database. A dedicated PostgreSQL session holds the lock
/// and is explicitly closed on shutdown (never returned to the pool locked).
pub async fn run(
    pool: PgPool,
    bot: Arc<dyn MorningBot>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    let worker = MorningWorker::new(pool.clone(), bot);
    loop {
        if *stop.borrow() {
            return;
        }
        let connection = pool.acquire().await;
        if let Ok(mut connection) = connection {
            let leader = sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock(734891302)")
                .fetch_one(&mut *connection)
                .await
                .unwrap_or(false);
            if leader {
                let polling = async {
                    let mut interval = tokio::time::interval(Duration::from_secs(2));
                    loop {
                        interval.tick().await;
                        if sqlx::query("SELECT 1")
                            .execute(&mut *connection)
                            .await
                            .is_err()
                        {
                            break;
                        }
                        if worker.poll().await.is_err() {
                            eprintln!("Telegram callback polling failed; will retry.");
                        }
                    }
                };
                let delivery = async {
                    let mut sweeps = tokio::time::interval(Duration::from_secs(30));
                    loop {
                        sweeps.tick().await;
                        if worker.deliver_due().await.is_err() {
                            eprintln!("Morning offer delivery failed; will retry pending offers.");
                        }
                    }
                };
                tokio::select! {
                    _ = stop.changed() => {},
                    _ = polling => {},
                    _ = delivery => {},
                }
            }
            let _ = connection.close().await;
        }
        if *stop.borrow() {
            return;
        }
        tokio::select! {
            _ = stop.changed() => return,
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_exact_positive_offer_choices() {
        assert_eq!(parse_choice("morning:42:yes"), Some((42, true)));
        assert_eq!(parse_choice("morning:42:no"), Some((42, false)));
        for bad in [
            "morning:0:yes",
            "morning:-1:no",
            "morning:1:yes:extra",
            "other:1:yes",
            "morning:1:maybe",
        ] {
            assert_eq!(parse_choice(bad), None);
        }
    }
}
