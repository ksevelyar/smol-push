use axum::body::Body;
use axum::http::Request;
use divan::Bencher;
use divan::counter::ItemsCount;
use futures_util::StreamExt;
use hyper::StatusCode;
use smol_push::delivery::DeliveryConfig;
use smol_push::queries::{self, NewPush, Platform};
use smol_push::utils::{TestDatabase, spawn_mock_fcm};
use smol_push::{AppState, app_from_state};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, mpsc};
use tower::ServiceExt;

const CONCURRENT_HTTP_REQUESTS: usize = 256;

fn main() {
    divan::main();
}

async fn spawn_mock_fcm_with_status(status: StatusCode) -> u16 {
    spawn_mock_fcm(status).await
}

#[divan::bench(sample_count = 10, args = [(1, 10000), (1, 20000), (1, 30000), (2, 30000), (3, 30000)])]
fn throughput(bencher: Bencher, &(max_connections, pushes_count): &(usize, usize)) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    bencher
        .counter(ItemsCount::new(pushes_count))
        .bench_local(|| {
            rt.block_on(async {
                let database = TestDatabase::new().await;
                let pool = database.pool().clone();
                let monitor_pool = pool.clone();

                let fcm_port = spawn_mock_fcm_with_status(StatusCode::OK).await;

                let config = DeliveryConfig {
                    android_address: format!("http://127.0.0.1:{fcm_port}"),
                    android_api_key: "test-key".into(),
                    max_connections,
                    max_concurrent_streams: 100,
                    max_retry_attempts: 0,
                    retry_base_delay_milliseconds: 1,
                    retry_max_delay_milliseconds: 1,
                };

                let app = smol_push::build_app(pool, None, 512, config);

                futures_util::stream::iter(0..pushes_count)
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

                loop {
                    let (pending,): (i64,) =
                        sqlx::query_as("SELECT COUNT(*) FROM pushes WHERE status = 0")
                            .fetch_one(&monitor_pool)
                            .await
                            .unwrap();

                    if pending == 0 {
                        break;
                    }

                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            });
        });
}

#[divan::bench(sample_count = 10, args = [(1, 30000), (2, 30000), (3, 30000)])]
fn delivery_only(bencher: Bencher, &(max_connections, pushes_count): &(usize, usize)) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    bencher
        .counter(ItemsCount::new(pushes_count))
        .bench_local(|| {
            rt.block_on(async {
                let database = TestDatabase::new().await;
                let pool = database.pool().clone();

                let pushes: Vec<NewPush> = (0..pushes_count)
                    .map(|i| NewPush {
                        id: uuid::Uuid::new_v4().to_string(),
                        platform: Platform::Android,
                        r#type: "info".into(),
                        text: "bench".into(),
                        token: format!("token-{i}"),
                        title: String::new(),
                    })
                    .collect();
                let refs: Vec<&NewPush> = pushes.iter().collect();
                queries::insert_batch(&refs, &pool).await;

                let fcm_port = spawn_mock_fcm_with_status(StatusCode::OK).await;

                let config = DeliveryConfig {
                    android_address: format!("http://127.0.0.1:{fcm_port}"),
                    android_api_key: "test-key".into(),
                    max_connections,
                    max_concurrent_streams: 100,
                    max_retry_attempts: 0,
                    retry_base_delay_milliseconds: 1,
                    retry_max_delay_milliseconds: 1,
                };

                smol_push::delivery::spawn_all(pool.clone(), config);

                loop {
                    let (pending,): (i64,) =
                        sqlx::query_as("SELECT COUNT(*) FROM pushes WHERE status = 0")
                            .fetch_one(&pool)
                            .await
                            .unwrap();

                    if pending == 0 {
                        break;
                    }

                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            });
        });
}

#[divan::bench(sample_count = 10, args = [1000, 10000, 30000])]
fn ingestion_only(bencher: Bencher, pushes_count: usize) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    bencher
        .counter(ItemsCount::new(pushes_count))
        .bench_local(|| {
            rt.block_on(async {
                let database = TestDatabase::new().await;
                let pool = database.pool().clone();
                let monitor_pool = pool.clone();

                let (writer_sender, writer_receiver) = mpsc::channel(512);
                let notify = Arc::new(Notify::new());
                smol_push::writer::spawn(writer_receiver, pool, notify);
                let state = Arc::new(AppState {
                    writer_sender,
                    api_key: None,
                });
                let app = app_from_state(state);

                futures_util::stream::iter(0..pushes_count)
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

                loop {
                    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM pushes")
                        .fetch_one(&monitor_pool)
                        .await
                        .unwrap();
                    if count as usize == pushes_count {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            });
        });
}
