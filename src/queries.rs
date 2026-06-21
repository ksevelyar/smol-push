use sqlx::{QueryBuilder, PgPool};


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
    Dispatching = 3,
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
    pub retry_count: i32,
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

pub async fn fetch_and_lock(pool: &PgPool, platform: Platform, limit: i32) -> Vec<Push> {
    sqlx::query_as::<_, Push>(
        "UPDATE pushes SET status = $1 WHERE id IN (\
            SELECT id FROM pushes WHERE platform = $2 AND status = $3 \
            AND (next_retry_at IS NULL OR next_retry_at <= EXTRACT(EPOCH FROM NOW())::bigint * 1000) \
            ORDER BY inserted_at ASC LIMIT $4\
        ) RETURNING *",
    )
    .bind(PushStatus::Dispatching)
    .bind(platform)
    .bind(PushStatus::Pending)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn reset_stale(pool: &PgPool) {
    if let Err(e) = sqlx::query("UPDATE pushes SET status = $1 WHERE status = $2")
        .bind(PushStatus::Pending)
        .bind(PushStatus::Dispatching)
        .execute(pool)
        .await
    {
        tracing::error!("reset_stale: {e}");
    }
}

pub async fn select_pending(pool: &PgPool, platform: Platform, limit: i32) -> Vec<Push> {
    sqlx::query_as::<_, Push>(
        "SELECT * FROM pushes WHERE platform = $1 AND status = $2 \
          AND (next_retry_at IS NULL OR next_retry_at <= EXTRACT(EPOCH FROM NOW())::bigint) \
          ORDER BY inserted_at ASC LIMIT $3",
    )
    .bind(platform)
    .bind(PushStatus::Pending)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn bulk_mark_status(pool: &PgPool, ids: &[String], status: PushStatus) {
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

pub async fn schedule_retry(pool: &PgPool, id: &str, next_retry_at: i64) {
    if let Err(e) = sqlx::query(
        "UPDATE pushes SET status = $1, retry_count = retry_count + 1, next_retry_at = $2 WHERE id = $3",
    )
    .bind(PushStatus::Pending)
    .bind(next_retry_at)
    .bind(id)
    .execute(pool)
    .await
    {
        tracing::error!("schedule_retry {id}: {e}");
    }
}

pub async fn insert_batch(batch: &[&NewPush], pool: &PgPool) {
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

pub async fn purge_old_pushes(pool: &PgPool) {
    if let Err(e) =
        sqlx::query("DELETE FROM pushes WHERE inserted_at < to_char(NOW() - INTERVAL '30 minutes', 'YYYY-MM-DD HH24:MI:SS')")
            .execute(pool)
            .await
    {
        tracing::error!("purge_old_pushes: {e}");
    }
}
