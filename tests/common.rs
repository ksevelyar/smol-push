use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http2::Builder;
use hyper::service::service_fn;
use hyper::{Request as HReq, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use sqlx::PgPool;

async fn recreate_database() -> PgPool {
    let admin_pool = PgPool::connect("postgres://postgres:postgres@localhost:5432/postgres")
        .await
        .expect("connect to admin pg");

    sqlx::query("DROP DATABASE IF EXISTS smol_push_test WITH (FORCE)")
        .execute(&admin_pool)
        .await
        .ok();
    sqlx::query("CREATE DATABASE smol_push_test")
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let pool = PgPool::connect("postgres://postgres:postgres@localhost:5432/smol_push_test")
        .await
        .unwrap();
    sqlx::migrate!().run(&pool).await.unwrap();
    pool
}

pub async fn create_test_database() -> PgPool {
    recreate_database().await
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
