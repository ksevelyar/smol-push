mod common;

use axum::body::Body;
use axum::http::Request;
use smol_push::delivery::DeliveryConfig;
use std::sync::Arc;
use tokio::sync::mpsc;
use tower::ServiceExt;

fn request(api_key: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/pushes")
        .header("content-type", "application/json")
        .header("api-key", api_key)
        .body(Body::from(body.to_owned()))
        .unwrap()
}

#[tokio::test]
async fn accept_push() {
    let application = smol_push::build_app(
        common::create_test_database().await,
        Some("key".into()),
        100,
        DeliveryConfig::default(),
    )
    .await;
    let status = application
        .oneshot(request(
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
    let application = smol_push::build_app(
        common::create_test_database().await,
        Some("key".into()),
        100,
        DeliveryConfig::default(),
    )
    .await;
    let status = application
        .oneshot(request(
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
    let application = smol_push::build_app(
        common::create_test_database().await,
        Some("key".into()),
        100,
        DeliveryConfig::default(),
    )
    .await;
    let status = application
        .oneshot(request("key", r#"{}"#))
        .await
        .unwrap()
        .status();
    assert_eq!(status, 422);
}

#[tokio::test]
async fn reject_backpressure() {
    let (sender, _receiver) = mpsc::channel(1);
    let (tx, _) = tokio::sync::oneshot::channel();
    let _ = sender.try_send(smol_push::writer::PushCommand {
        payload: smol_push::queries::NewPush {
            id: "fill".into(),
            platform: smol_push::queries::Platform::Android,
            r#type: "alert".into(),
            text: "fill".into(),
            token: String::new(),
            title: String::new(),
        },
        acknowledgement: tx,
    });
    let state = Arc::new(smol_push::AppState {
        writer_sender: sender,
        api_key: Some("key".into()),
    });
    let application = smol_push::app_from_state(state);
    let status = application
        .oneshot(request(
            "key",
            r#"{"platform":"android","type":"alert","text":"full"}"#,
        ))
        .await
        .unwrap()
        .status();
    assert_eq!(status, 429);
}
