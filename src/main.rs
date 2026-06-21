use smol_push::delivery::DeliveryConfig;
use smol_push::queries;
use sqlx::PgPool;

fn environment_variable(key: &str) -> String {
    std::env::var(key).expect("missing env var, set in flake.nix devShell")
}

#[tokio::main]
async fn main() {
    console_subscriber::init();

    let database_url = environment_variable("DATABASE_URL");

    let pool = PgPool::connect(&database_url)
        .await
        .expect("connect to postgres");

    sqlx::migrate!().run(&pool).await.expect("run migration");

    queries::purge_old_pushes(&pool).await;

    let apple_key = Some(environment_variable("PUSH_API_KEY"));

    let configuration = DeliveryConfig {
        android_address: environment_variable("ANDROID_ADDRESS"),
        android_api_key: environment_variable("ANDROID_API_KEY"),
        max_connections: environment_variable("MAX_CONNECTIONS_PER_PROVIDER")
            .parse()
            .unwrap(),
        max_concurrent_streams: environment_variable("MAX_CONCURRENT_STREAMS")
            .parse()
            .unwrap(),
        max_retry_attempts: environment_variable("MAX_RETRY_ATTEMPTS").parse().unwrap(),
        retry_base_delay_milliseconds: environment_variable("RETRY_BASE_DELAY_MS").parse().unwrap(),
        retry_max_delay_milliseconds: environment_variable("RETRY_MAX_DELAY_MS").parse().unwrap(),
    };

    let maximum_queued: usize = environment_variable("MAX_QUEUED_PUSHES").parse().unwrap();
    let application = smol_push::build_app(pool, apple_key, maximum_queued, configuration);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4004")
        .await
        .expect("bind listener");

    tracing::info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, application).await.unwrap();
}
