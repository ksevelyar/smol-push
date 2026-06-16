use sqlx::{QueryBuilder, SqlitePool};

#[derive(serde::Deserialize, sqlx::Type, Clone, Copy)]
#[serde(rename_all = "lowercase")]
#[repr(i32)]
pub enum Platform {
    Apple = 0,
    Android = 1,
}

#[derive(Debug, Clone, Copy, sqlx::Type)]
#[repr(i32)]
pub enum PushStatus {
    Pending = 0,
    Delivered = 1,
    Dead = 2,
}

#[derive(sqlx::FromRow)]
pub struct Push {
    pub id: String,
    pub platform: Platform,
    pub r#type: String,
    pub text: String,
    pub token: String,
    pub title: String,
    pub inserted_at: String,
    pub retry_count: u8,
    pub next_retry_at: Option<i64>,
    pub status: PushStatus,
}

pub struct NewPush {
    pub id: String,
    pub platform: Platform,
    pub r#type: String,
    pub text: String,
    pub token: String,
    pub title: String,
}

pub enum PushResult {
    Delivered,
    Fatal,
    RecoverableError,
}

pub async fn select_pending(pool: &SqlitePool, platform: Platform, limit: i32) -> Vec<Push> {
    sqlx::query_as::<_, Push>(
        "SELECT * FROM pushes WHERE platform = ? AND status = ? \
          AND (next_retry_at IS NULL OR next_retry_at <= CAST(strftime('%s', 'now') AS INTEGER)) \
          ORDER BY inserted_at ASC LIMIT ?",
    )
    .bind(platform)
    .bind(PushStatus::Pending)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn bulk_mark_status(pool: &SqlitePool, ids: &[String], status: PushStatus) {
    if ids.is_empty() {
        return;
    }
    let mut qb = QueryBuilder::new("UPDATE pushes SET status = ");
    qb.push_bind(status);
    qb.push(" WHERE id IN (");
    let mut sep = qb.separated(", ");
    for id in ids {
        sep.push_bind(id);
    }
    qb.push(")");
    if let Err(e) = qb.build().execute(pool).await {
        tracing::error!("bulk_mark_status (status={status:?}): {e}");
    }
}

pub async fn schedule_retry(pool: &SqlitePool, id: &str, next_retry_at: i64) {
    if let Err(e) = sqlx::query(
        "UPDATE pushes SET retry_count = retry_count + 1, next_retry_at = ? WHERE id = ?",
    )
    .bind(next_retry_at)
    .bind(id)
    .execute(pool)
    .await
    {
        tracing::error!("mark_transient {id}: {e}");
    }
}

pub async fn insert_batch(batch: &[&NewPush], pool: &SqlitePool) {
    let mut builder =
        QueryBuilder::new("INSERT INTO pushes (id, platform, type, text, token, title) ");

    builder.push_values(batch, |mut b, p| {
        b.push_bind(&p.id)
            .push_bind(p.platform)
            .push_bind(&p.r#type)
            .push_bind(&p.text)
            .push_bind(&p.token)
            .push_bind(&p.title);
    });

    if let Err(e) = builder.build().execute(pool).await {
        tracing::error!("insert_batch: {e}");
    }
}

pub async fn purge_old_pushes(pool: &SqlitePool) {
    if let Err(e) =
        sqlx::query("DELETE FROM pushes WHERE inserted_at < datetime('now', '-30 minutes')")
            .execute(pool)
            .await
    {
        tracing::error!("purge_old_pushes: {e}");
    }
}
