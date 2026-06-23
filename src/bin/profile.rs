use axum::body::Body;
use axum::http::Request;
use futures_util::StreamExt;
use hyper::StatusCode;
use smol_push::delivery::DeliveryConfig;
use smol_push::utils::{TestDatabase, spawn_mock_fcm};
use std::time::{Duration, Instant};
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const PUSHES: usize = 10000;
const CONCURRENT_HTTP_REQUESTS: usize = 256;
const MAX_CONNECTIONS: usize = 1;

#[tokio::main]
async fn main() {
    let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
        .file("trace.json")
        .include_args(true)
        .build();

    tracing_subscriber::registry()
        .with(chrome_layer)
        .with(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let database = TestDatabase::new().await;
    let pool = database.pool().clone();

    let fcm_port = spawn_mock_fcm(StatusCode::OK).await;

    let config = DeliveryConfig {
        android_address: format!("http://127.0.0.1:{fcm_port}"),
        android_api_key: "test-key".into(),
        max_connections: MAX_CONNECTIONS,
        max_concurrent_streams: 100,
        max_retry_attempts: 0,
        retry_base_delay_milliseconds: 1,
        retry_max_delay_milliseconds: 1,
    };

    let app = smol_push::build_app(pool.clone(), None, 512, config);

    let start = Instant::now();

    futures_util::stream::iter(0..PUSHES)
        .for_each_concurrent(CONCURRENT_HTTP_REQUESTS, |_| {
            let app = app.clone();
            async move {
                app.oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/pushes")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"platform":"android","type":"info","text":"bench"}"#,
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            }
        })
        .await;

    let monitor_pool = pool.clone();

    loop {
        let (pending,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM pushes WHERE status = 0")
            .fetch_one(&monitor_pool)
            .await
            .unwrap();

        if pending == 0 {
            break;
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let elapsed = start.elapsed();
    println!(
        "Delivered {PUSHES} pushes in {elapsed:?} ({:.1} K/s)",
        PUSHES as f64 / elapsed.as_secs_f64() / 1000.0
    );

    drop(guard);
}
