mod common;

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::Request;
use hyper::StatusCode;
use smol_push::delivery::DeliveryConfig;
use tower::ServiceExt;

#[tokio::test]
async fn benchmark_delivery_throughput() {
    const TOTAL_REQUESTS: usize = 10_000;

    let fcm_port = common::spawn_mock_fcm(StatusCode::OK).await;
    tokio::time::sleep(Duration::from_millis(1)).await;

    let pool = common::create_test_database().await;
    let monitor_pool = pool.clone();

    let configuration = DeliveryConfig {
        android_address: format!("http://127.0.0.1:{fcm_port}"),
        android_api_key: "test-key".into(),
        max_connections: 1,
        max_retry_attempts: 0,
        retry_base_delay_milliseconds: 1,
        retry_max_delay_milliseconds: 1,
    };

    let app = smol_push::build_app(pool, None, 100_000, configuration);

    let start = Instant::now();

    let mut handles = Vec::with_capacity(TOTAL_REQUESTS);
    for _ in 0..TOTAL_REQUESTS {
        let app = app.clone();
        handles.push(tokio::spawn(async move {
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
        }));
    }

    for (i, handle) in handles.into_iter().enumerate() {
        let resp = handle.await.unwrap().unwrap();
        assert!(resp.status() == 200, "push {i}: {}", resp.status());
    }

    let ingest = start.elapsed();

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

    let total = start.elapsed();
    let delivery = total.checked_sub(ingest).unwrap();

    println!();
    println!("=== BENCHMARK ===");
    println!("pushes:          {TOTAL_REQUESTS}");
    println!(
        "ingest:          {ingest:?}  ({:>8.0}/s)",
        TOTAL_REQUESTS as f64 / ingest.as_secs_f64()
    );
    println!(
        "delivery:        {delivery:?}  ({:>8.0}/s)",
        TOTAL_REQUESTS as f64 / delivery.as_secs_f64()
    );
    println!(
        "total:           {total:?}  ({:>8.0}/s)",
        TOTAL_REQUESTS as f64 / total.as_secs_f64()
    );

    drop(monitor_pool);
}
