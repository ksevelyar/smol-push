pub mod delivery;
pub mod queries;
pub mod writer;

use axum::{
    Router,
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use delivery::DeliveryConfig;
use queries::{NewPush, Platform};
use serde::Deserialize;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::{Notify, mpsc, oneshot};
use writer::PushCommand;

#[derive(Deserialize)]
pub struct PushRequest {
    pub platform: Platform,
    pub r#type: String,
    pub text: String,
    pub token: Option<String>,
    pub title: Option<String>,
}

pub struct AppState {
    pub writer_sender: mpsc::Sender<PushCommand>,
    pub api_key: Option<String>,
}

pub fn app_from_state(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/pushes", post(create_push))
        .with_state(state)
}

pub async fn build_app(
    pool: SqlitePool,
    api_key: Option<String>,
    max_queued: usize,
    delivery_config: DeliveryConfig,
) -> Router {
    let (writer_sender, writer_receiver) = mpsc::channel(max_queued.max(1));
    let notify = Arc::new(Notify::new());

    writer::spawn(writer_receiver, pool.clone(), Arc::clone(&notify));

    delivery::spawn(pool, notify, delivery_config);

    let state = Arc::new(AppState {
        writer_sender,
        api_key,
    });

    app_from_state(state)
}

pub async fn create_push(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<PushRequest>,
) -> impl IntoResponse {
    if let Some(ref key) = state.api_key {
        let header = headers
            .get("api-key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if header != key {
            return StatusCode::UNAUTHORIZED;
        }
    }

    let push = NewPush {
        id: uuid::Uuid::new_v4().to_string(),
        platform: body.platform,
        r#type: body.r#type,
        text: body.text,
        token: body.token.unwrap_or_default(),
        title: body.title.unwrap_or_default(),
    };

    let (acknowledgement_sender, acknowledgement_receiver) = oneshot::channel();
    let command = PushCommand {
        payload: push,
        acknowledgement: acknowledgement_sender,
    };

    match state.writer_sender.try_send(command) {
        Ok(_) => {
            if acknowledgement_receiver.await.is_err() {
                tracing::error!("writer dropped the acknowledgement channel");
            }
            StatusCode::OK
        }
        Err(mpsc::error::TrySendError::Full(_)) => StatusCode::TOO_MANY_REQUESTS,
        Err(mpsc::error::TrySendError::Closed(_)) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
