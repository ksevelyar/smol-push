use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http2::Builder;
use hyper::service::service_fn;
use hyper::{Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use sqlx::SqlitePool;
use uuid::Uuid;

pub struct TestDatabase {
    pool: Option<SqlitePool>,
    path: std::path::PathBuf,
}

impl TestDatabase {
    pub async fn new() -> Self {
        let path = std::env::temp_dir().join(format!("smol_push_{}.db", Uuid::new_v4()));

        let database_url = format!("sqlite:{}?mode=rwc", path.display());

        let pool = SqlitePool::connect(&database_url).await.unwrap();

        sqlx::query("PRAGMA journal_mode = WAL;")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("PRAGMA synchronous = NORMAL;")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();

        Self {
            pool: Some(pool),
            path,
        }
    }

    pub fn pool(&self) -> &SqlitePool {
        self.pool.as_ref().unwrap()
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        drop(self.pool.take());
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", self.path.display(), suffix));
        }
    }
}

pub async fn spawn_mock_fcm(status_code: StatusCode) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            stream.set_nodelay(true).ok();
            let io = TokioIo::new(stream);
            tokio::spawn(async move {
                let service = service_fn(move |_| {
                    let body = Full::new(Bytes::from_static(b"{}"));
                    async move {
                        Ok::<_, hyper::Error>(
                            Response::builder().status(status_code).body(body).unwrap(),
                        )
                    }
                });
                let _ = Builder::new(TokioExecutor::new())
                    .serve_connection(io, service)
                    .await;
            });
        }
    });

    port
}
