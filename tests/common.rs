use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http2::Builder;
use hyper::service::service_fn;
use hyper::{Request as HReq, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use sqlx::SqlitePool;
use uuid::Uuid;

pub async fn create_test_database() -> SqlitePool {
    let path = std::env::temp_dir().join(format!("smol_push_{}.db", Uuid::new_v4()));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let pool = SqlitePool::connect(&url).await.unwrap();
    sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("PRAGMA synchronous = NORMAL;")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::migrate!().run(&pool).await.unwrap();
    pool
}

#[allow(dead_code)]
pub async fn spawn_mock_fcm(status: StatusCode) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            stream.set_nodelay(true).ok();
            let io = TokioIo::new(stream);
            tokio::spawn(async move {
                let svc = service_fn(move |_req: HReq<hyper::body::Incoming>| {
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
