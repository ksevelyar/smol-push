use axum::body::Body;
use axum::http::Request;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tokio::sync::mpsc;
use tower::ServiceExt;

async fn pool() -> SqlitePool {
    let p = SqlitePool::connect(":memory:").await.unwrap();
    sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(&p)
        .await
        .unwrap();
    sqlx::migrate!().run(&p).await.unwrap();
    p
}

fn req(key: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/pushes")
        .header("content-type", "application/json")
        .header("api-key", key)
        .body(Body::from(body.to_owned()))
        .unwrap()
}

#[tokio::test]
async fn accept_push() {
    let app = smol_push::build_app(pool().await, Some("key".into()), 100).await;
    let status = app
        .oneshot(req(
            "key",
            r#"{"platform":"apple","type":"info","text":"hi"}"#,
        ))
        .await
        .unwrap()
        .status();
    assert_eq!(status, 200);
}

#[tokio::test]
async fn reject_unauthorized() {
    let app = smol_push::build_app(pool().await, Some("key".into()), 100).await;
    let status = app
        .oneshot(req(
            "wrong",
            r#"{"platform":"apple","type":"alert","text":"no"}"#,
        ))
        .await
        .unwrap()
        .status();
    assert_eq!(status, 401);
}

#[tokio::test]
async fn reject_malformed_body() {
    let app = smol_push::build_app(pool().await, Some("key".into()), 100).await;
    let status = app.oneshot(req("key", r#"{}"#)).await.unwrap().status();
    assert_eq!(status, 422);
}

#[tokio::test]
async fn reject_backpressure() {
    let (tx, _rx) = mpsc::channel(10);
    let pending = Arc::new(AtomicUsize::new(1));
    let state = Arc::new(smol_push::AppState {
        writer_tx: tx,
        pending,
        api_key: Some("key".into()),
        max_queued: 1,
    });
    let app = smol_push::app_from_state(state);
    let status = app
        .oneshot(req(
            "key",
            r#"{"platform":"android","type":"alert","text":"full"}"#,
        ))
        .await
        .unwrap()
        .status();
    assert_eq!(status, 429);
}
