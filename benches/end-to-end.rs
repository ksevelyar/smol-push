use axum::body::Body;
use axum::http::Request;
use bytes::Bytes;
use divan::Bencher;
use divan::counter::ItemsCount;
use http_body_util::Full;
use hyper::server::conn::http2::Builder;
use hyper::service::service_fn;
use hyper::{Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use smol_push::delivery::DeliveryConfig;
use sqlx::PgPool;
use std::time::Duration;
use tokio::task::JoinSet;
use tower::ServiceExt;

fn main() {
    divan::main();
}

async fn create_test_database() -> PgPool {
    let url = "postgres://postgres:postgres@localhost:5432/smol_push";
    let pool = PgPool::connect(url).await.unwrap();
    sqlx::migrate!().run(&pool).await.unwrap();
    sqlx::query("DELETE FROM pushes").execute(&pool).await.unwrap();
    pool
}

async fn spawn_mock_fcm(status: StatusCode) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            stream.set_nodelay(true).ok();
            let io = TokioIo::new(stream);
            tokio::spawn(async move {
                let svc = service_fn(move |_| {
                    let body = Full::new(Bytes::from_static(b"{}"));
                    async move {
                        Ok::<_, hyper::Error>(
                            Response::builder().status(status).body(body).unwrap(),
                        )
                    }
                });
                let _ = Builder::new(TokioExecutor::new())
                    .serve_connection(io, svc)
                    .await;
            });
        }
    });

    port
}

#[divan::bench(sample_count = 10, args = [(1, 1000), (1, 10000), (1, 20000), (1, 50000), (2, 50000), (3, 50000)])]
fn throughput(bencher: Bencher, &(max_connections, pushes_count): &(usize, usize)) {
    bencher
        .counter(ItemsCount::new(pushes_count))
        .bench_local(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let pool = create_test_database().await;
                let monitor_pool = pool.clone();

                let fcm_port = spawn_mock_fcm(StatusCode::OK).await;

                let config = DeliveryConfig {
                    android_address: format!("http://127.0.0.1:{fcm_port}"),
                    android_api_key: "test-key".into(),
                    max_connections,
                    max_concurrent_streams: 100,
                    max_retry_attempts: 0,
                    retry_base_delay_milliseconds: 1,
                    retry_max_delay_milliseconds: 1,
                };

                let app = smol_push::build_app(pool, None, 100_000, config);

                let mut set = JoinSet::new();

                for _ in 0..pushes_count {
                    let app = app.clone();

                    set.spawn(async move {
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
                    });

                    if set.len() >= 256 {
                        set.join_next().await.unwrap().unwrap();
                    }
                }

                while set.join_next().await.is_some() {}

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
