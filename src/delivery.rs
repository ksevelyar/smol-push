pub mod android_worker;

use crate::queries;
use sqlx::SqlitePool;

pub use queries::Push;

#[derive(Clone)]
pub struct DeliveryConfig {
    pub max_connections: usize,
    pub max_concurrent_streams: usize,
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
            max_concurrent_streams: 100,
            max_retry_attempts: 3,
            retry_base_delay_milliseconds: 1000,
            retry_max_delay_milliseconds: 60000,
            android_address: String::new(),
            android_api_key: String::new(),
        }
    }
}

pub fn spawn_all(pool: SqlitePool, configuration: DeliveryConfig) {
    tokio::spawn({
        let pool = pool.clone();
        async move { queries::reset_stale(&pool).await }
    });

    for worker_index in 0..configuration.max_connections {
        android_worker::spawn(
            pool.clone(),
            configuration.clone(),
            worker_index,
        );
    }
}
