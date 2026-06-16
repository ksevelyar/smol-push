pub mod android;

use crate::queries::{self, Platform, PushResult, PushStatus};
use futures_util::future;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Notify;

const BATCH_SIZE: i32 = 100;

pub struct DeliveryConfig {
    pub max_connections: usize, // TODO: implement connection pool
    pub max_retry_attempts: u8,
    pub retry_base_delay_milliseconds: u64,
    pub retry_max_delay_milliseconds: u64,
    pub android_address: String,
    pub android_api_key: String,
}

impl Default for DeliveryConfig {
    fn default() -> Self {
        Self {
            max_connections: 1,
            max_retry_attempts: 3,
            retry_base_delay_milliseconds: 1000,
            retry_max_delay_milliseconds: 60000,
            android_address: String::new(),
            android_api_key: String::new(),
        }
    }
}

pub fn spawn(pool: SqlitePool, notify: Arc<Notify>, configuration: DeliveryConfig) {
    tokio::spawn(async move {
        android_task(pool, notify, configuration).await;
    });
}

async fn android_task(pool: SqlitePool, notify: Arc<Notify>, configuration: DeliveryConfig) {
    loop {
        let connection = match android::AndroidConnection::connect(
            &configuration.android_address,
            &configuration.android_api_key,
        )
        .await
        {
            Ok(connection) => connection,
            Err(e) => {
                tracing::error!("android connection failed, retrying in 15 seconds: {e}");
                tokio::time::sleep(Duration::from_secs(15)).await;
                continue;
            }
        };

        loop {
            let pushes = queries::select_pending(&pool, Platform::Android, BATCH_SIZE).await;

            if pushes.is_empty() {
                notify.notified().await;
                continue;
            }

            let mut delivered = Vec::with_capacity(pushes.len());
            let mut dead = Vec::with_capacity(pushes.len());

            for (id, outcome, retry_count) in future::join_all(pushes.into_iter().map(|push| {
                let mut sender = connection.sender.clone();
                let api_key = connection.api_key.clone();
                async move {
                    let outcome = android::send_notification(
                        &mut sender,
                        &api_key,
                        &push.token,
                        &push.title,
                        &push.text,
                    )
                    .await;
                    (push.id, outcome, push.retry_count)
                }
            }))
            .await
            {
                match outcome {
                    PushResult::Delivered => delivered.push(id),
                    PushResult::Fatal => dead.push(id),
                    PushResult::RecoverableError => {
                        if retry_count >= configuration.max_retry_attempts {
                            dead.push(id);
                        } else {
                            let delay = retry_delay(
                                retry_count,
                                configuration.retry_base_delay_milliseconds,
                                configuration.retry_max_delay_milliseconds,
                            );
                            let next_at = SystemTime::now() + delay;
                            let ts = next_at
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap()
                                .as_secs() as i64;
                            queries::schedule_retry(&pool, &id, ts).await;
                        }
                    }
                }
            }

            queries::bulk_mark_status(&pool, &delivered, PushStatus::Delivered).await;
            queries::bulk_mark_status(&pool, &dead, PushStatus::Dead).await;
        }
    }
}

fn retry_delay(retry_count: u8, base_ms: u64, max_ms: u64) -> Duration {
    Duration::from_millis(base_ms * 2u64.pow(retry_count.into()).min(max_ms))
}
