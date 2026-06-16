mod common;

use axum::body::Body;
use axum::http::Request;
use hyper::StatusCode;
use smol_push::delivery::DeliveryConfig;
use smol_push::queries::PushStatus;
use std::time::Duration;
use tower::ServiceExt;

async fn run_delivery_test(
    mock_status: StatusCode,
    expected_status: PushStatus,
    expected_retry_count: i32,
    initial_retry_count: i32,
    max_retries: i32,
) {
    let pool = common::create_test_database().await;
    let monitor_pool = pool.clone();
    if initial_retry_count > 0 {
        sqlx::query(
            "INSERT INTO pushes (id, platform, type, text, status, retry_count) \
             VALUES ('t1', 1, 'info', 'hello', 0, ?)",
        )
        .bind(initial_retry_count)
        .execute(&pool)
        .await
        .unwrap();
    }

    let fcm_port = common::spawn_mock_fcm(mock_status).await;
    tokio::time::sleep(Duration::from_millis(1)).await;

    let config = DeliveryConfig {
        android_address: format!("http://127.0.0.1:{fcm_port}"),
        android_api_key: "test-key".into(),
        max_connections: 1,
        max_retry_attempts: max_retries as u8,
        retry_base_delay_milliseconds: 1,
        retry_max_delay_milliseconds: 1,
    };

    let app = smol_push::build_app(pool, None, 100, config).await;

    let id = if initial_retry_count > 0 {
        "t1".to_string()
    } else {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pushes")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"platform":"android","type":"info","text":"hello"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let (id,): (String,) = sqlx::query_as("SELECT id FROM pushes")
            .fetch_one(&monitor_pool)
            .await
            .unwrap();
        id
    };

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some((status, retry)) =
                sqlx::query_as::<_, (i32, i32)>("SELECT status, retry_count FROM pushes WHERE id = ?")
                    .bind(&id)
                    .fetch_optional(&monitor_pool)
                    .await
                    .unwrap()
            {
                if status == expected_status as i32 && retry == expected_retry_count {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!("push did not reach expected status={expected_status:?} retry_count={expected_retry_count}")
    });
}

#[tokio::test]
async fn deliver_success_marks_delivered() {
    run_delivery_test(StatusCode::OK, PushStatus::Delivered, 0, 0, 3).await;
}

#[tokio::test]
async fn deliver_fatal_marks_dead() {
    run_delivery_test(StatusCode::BAD_REQUEST, PushStatus::Dead, 0, 0, 3).await;
}

#[tokio::test]
async fn deliver_recoverable_increments_retry() {
    run_delivery_test(
        StatusCode::INTERNAL_SERVER_ERROR,
        PushStatus::Pending,
        1,
        0,
        3,
    )
    .await;
}

#[tokio::test]
async fn deliver_max_retries_exceeded_marks_dead() {
    run_delivery_test(StatusCode::INTERNAL_SERVER_ERROR, PushStatus::Dead, 3, 3, 3).await;
}
