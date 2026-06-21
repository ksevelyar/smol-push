use crate::queries::{self, NewPush};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, mpsc, oneshot};

pub struct PushCommand {
    pub payload: NewPush,
    pub acknowledgement: oneshot::Sender<()>,
}

pub fn spawn(mut receiver: mpsc::Receiver<PushCommand>, pool: PgPool, notify: Arc<Notify>) {
    tokio::spawn(async move {
        loop {
            let batch = collect(&mut receiver, 100, Duration::from_millis(5)).await;
            if batch.is_empty() {
                continue;
            }

            flush(&batch, &pool).await;
            notify.notify_one();

            for command in batch {
                let _ = command.acknowledgement.send(());
            }
        }
    });
}

async fn collect(
    receiver: &mut mpsc::Receiver<PushCommand>,
    maximum: usize,
    timeout: Duration,
) -> Vec<PushCommand> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut batch = Vec::with_capacity(maximum);

    match tokio::time::timeout(timeout, receiver.recv()).await {
        Ok(Some(command)) => batch.push(command),
        _ => return batch,
    }

    while batch.len() < maximum && tokio::time::Instant::now() < deadline {
        match receiver.try_recv() {
            Ok(command) => batch.push(command),
            Err(_) => break,
        }
    }

    batch
}

async fn flush(batch: &[PushCommand], pool: &PgPool) {
    let payloads: Vec<&NewPush> = batch.iter().map(|c| &c.payload).collect();
    queries::insert_batch(&payloads, pool).await;
}
