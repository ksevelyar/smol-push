use sqlx::SqlitePool;

#[tokio::main]
async fn main() {
    console_subscriber::init();

    let pool = SqlitePool::connect("sqlite:pushes.db?mode=rwc")
        .await
        .expect("connect to sqlite");

    sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(&pool)
        .await
        .expect("enable WAL");
    sqlx::query("PRAGMA synchronous = NORMAL;")
        .execute(&pool)
        .await
        .expect("set synchronous mode");

    sqlx::migrate!().run(&pool).await.expect("run migration");

    sqlx::query("DELETE FROM pushes WHERE inserted_at < datetime('now', '-30 minutes')")
        .execute(&pool)
        .await
        .expect("purge old pushes");

    let max_queued: usize = std::env::var("MAX_QUEUED_PUSHES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);

    let api_key = std::env::var("PUSH_API_KEY").ok();

    let app = smol_push::build_app(pool, api_key, max_queued).await;

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4004")
        .await
        .expect("bind listener");

    tracing::info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
