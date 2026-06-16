pub mod writer;

use axum::{
    Router,
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use serde::Deserialize;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{mpsc, oneshot};
use writer::{Push, PushCmd};

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Apple = 0,
    Android = 1,
}

impl Platform {
    pub fn as_db_int(&self) -> i32 {
        *self as i32
    }
}

#[derive(Debug, Deserialize)]
pub struct PushRequest {
    pub platform: Platform,
    pub r#type: String,
    pub text: String,
}

pub struct AppState {
    pub writer_tx: mpsc::Sender<PushCmd>,
    pub pending: Arc<AtomicUsize>,
    pub api_key: Option<String>,
    pub max_queued: usize,
}

pub fn app_from_state(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/pushes", post(post_push))
        .with_state(state)
}

pub async fn build_app(pool: SqlitePool, api_key: Option<String>, max_queued: usize) -> Router {
    let (writer_tx, writer_rx) = mpsc::channel(max_queued.max(1));
    let pending = Arc::new(AtomicUsize::new(0));
    writer::spawn(writer_rx, pool, Arc::clone(&pending));

    let state = Arc::new(AppState {
        writer_tx,
        pending,
        api_key,
        max_queued,
    });

    app_from_state(state)
}

pub async fn post_push(
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

    if state.pending.load(Ordering::SeqCst) >= state.max_queued {
        return StatusCode::TOO_MANY_REQUESTS;
    }

    let push = Push {
        id: uuid::Uuid::new_v4().to_string(),
        platform: body.platform.as_db_int(),
        r#type: body.r#type,
        text: body.text,
    };

    let (ack_tx, ack_rx) = oneshot::channel();
    let cmd = PushCmd { push, ack: ack_tx };

    match state.writer_tx.try_send(cmd) {
        Ok(_) => {
            state.pending.fetch_add(1, Ordering::SeqCst);
            let _ = ack_rx.await;
            StatusCode::OK
        }
        Err(mpsc::error::TrySendError::Full(_)) => StatusCode::TOO_MANY_REQUESTS,
        Err(mpsc::error::TrySendError::Closed(_)) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
