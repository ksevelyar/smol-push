use axum::body::Body;
use axum::http::Request;
use http_body_util::Full;
use hyper::server::conn::http2::Builder;
use hyper::service::service_fn;
use hyper::{Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use smol_push::delivery::DeliveryConfig;
use sqlx::PgPool;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const PUSHES: usize = 10000;
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

    let pool = create_test_database().await;

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

    let app = smol_push::build_app(pool.clone(), None, 100_000, config);

    let mut set = JoinSet::new();

    for _ in 0..PUSHES {
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

    let start = Instant::now();
    let monitor_pool = pool.clone();

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

    let elapsed = start.elapsed();
    println!("Delivered {PUSHES} pushes in {elapsed:?} ({:.1} K/s)", PUSHES as f64 / elapsed.as_secs_f64() / 1000.0);

    drop(guard);
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
                    let body = Full::new(bytes::Bytes::from_static(b"{}"));
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
