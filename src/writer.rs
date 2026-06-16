use sqlx::SqlitePool;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

pub struct Push {
    pub id: String,
    pub platform: i32,
    pub r#type: String,
    pub text: String,
}

pub struct PushCmd {
    pub push: Push,
    pub ack: oneshot::Sender<()>,
}

pub fn spawn(mut rx: mpsc::Receiver<PushCmd>, pool: SqlitePool, pending: Arc<AtomicUsize>) {
    tokio::spawn(async move {
        loop {
            let batch = collect(&mut rx, 100, Duration::from_millis(5)).await;
            if batch.is_empty() {
                continue;
            }

            flush(&batch, &pool).await;

            for cmd in batch {
                let _ = cmd.ack.send(());
                pending.fetch_sub(1, Ordering::SeqCst);
            }
        }
    });
}

async fn collect(rx: &mut mpsc::Receiver<PushCmd>, max: usize, timeout: Duration) -> Vec<PushCmd> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut batch = Vec::with_capacity(max);

    match tokio::time::timeout(timeout, rx.recv()).await {
        Ok(Some(cmd)) => batch.push(cmd),
        _ => return batch,
    }

    while batch.len() < max && tokio::time::Instant::now() < deadline {
        match rx.try_recv() {
            Ok(cmd) => batch.push(cmd),
            Err(_) => break,
        }
    }

    batch
}

async fn flush(batch: &[PushCmd], pool: &SqlitePool) {
    let params: Vec<String> = batch.iter().map(|_| "(?, ?, ?, ?)".to_string()).collect();
    let sql = format!(
        "INSERT INTO pushes (id, platform, type, text) VALUES {}",
        params.join(", ")
    );

    let mut query = sqlx::query(&sql);
    for cmd in batch {
        query = query
            .bind(&cmd.push.id)
            .bind(cmd.push.platform)
            .bind(&cmd.push.r#type)
            .bind(&cmd.push.text);
    }

    if let Err(e) = query.execute(pool).await {
        tracing::error!("flush: {e}");
    }
}
