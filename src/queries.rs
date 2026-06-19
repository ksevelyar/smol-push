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

#[derive(sqlx::FromRow, Debug)]
pub struct ExplainRow {
    pub id: i32,
    pub parent: i32,
    pub detail: String,
}

pub async fn explain(pool: &SqlitePool, sql: &str) -> Vec<ExplainRow> {
    let sql = format!("EXPLAIN QUERY PLAN {sql}");
    sqlx::query_as::<_, ExplainRow>(&sql)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
}

pub const FETCH_AND_LOCK_SQL: &str = r#"
UPDATE pushes
SET status = ?
WHERE id IN (
    SELECT id
    FROM pushes
    WHERE platform = ?
      AND status = ?
      AND (
          next_retry_at IS NULL OR
          next_retry_at <= CAST(strftime('%s', 'now') AS INTEGER) * 1000
      )
    ORDER BY inserted_at ASC
    LIMIT ?
)
RETURNING *
"#;

pub const SELECT_PENDING_SQL: &str = "SELECT * FROM pushes WHERE platform = ? AND status = ? \
     AND (next_retry_at IS NULL OR next_retry_at <= CAST(strftime('%s', 'now') AS INTEGER)) \
     ORDER BY inserted_at ASC LIMIT ?";

pub const RESET_STALE_SQL: &str = "UPDATE pushes SET status = ? WHERE status = ?";

pub const SCHEDULE_RETRY_SQL: &str =
    "UPDATE pushes SET status = ?, retry_count = retry_count + 1, next_retry_at = ? WHERE id = ?";

pub const PURGE_OLD_PUSHES_SQL: &str =
    "DELETE FROM pushes WHERE inserted_at < datetime('now', '-30 minutes')";

pub async fn fetch_and_lock(pool: &SqlitePool, platform: Platform, limit: i32) -> Vec<Push> {
    sqlx::query_as::<_, Push>(FETCH_AND_LOCK_SQL)
        .bind(PushStatus::Dispatching)
        .bind(platform)
        .bind(PushStatus::Pending)
        .bind(limit)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
}

pub async fn reset_stale(pool: &SqlitePool) {
    if let Err(e) = sqlx::query(RESET_STALE_SQL)
        .bind(PushStatus::Pending)
        .bind(PushStatus::Dispatching)
        .execute(pool)
        .await
    {
        tracing::error!("reset_stale: {e}");
    }
}

pub async fn select_pending(pool: &SqlitePool, platform: Platform, limit: i32) -> Vec<Push> {
    sqlx::query_as::<_, Push>(SELECT_PENDING_SQL)
        .bind(platform)
        .bind(PushStatus::Pending)
        .bind(limit)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
}

pub fn build_bulk_mark_status(
    ids: &[String],
    status: PushStatus,
) -> Option<QueryBuilder<'_, sqlx::Sqlite>> {
    if ids.is_empty() {
        return None;
    }
    let mut qb = QueryBuilder::new("UPDATE pushes SET status = ");
    qb.push_bind(status);
    qb.push(" WHERE id IN (");
    let mut sep = qb.separated(", ");
    for id in ids {
        sep.push_bind(id);
    }
    qb.push(")");
    Some(qb)
}

pub async fn bulk_mark_status(pool: &SqlitePool, ids: &[String], status: PushStatus) {
    let Some(mut qb) = build_bulk_mark_status(ids, status) else {
        return;
    };
    if let Err(e) = qb.build().execute(pool).await {
        tracing::error!("bulk_mark_status (status={status:?}): {e}");
    }
}

pub async fn schedule_retry(pool: &SqlitePool, id: &str, next_retry_at: i64) {
    if let Err(e) = sqlx::query(SCHEDULE_RETRY_SQL)
        .bind(PushStatus::Pending)
        .bind(next_retry_at)
        .bind(id)
        .execute(pool)
        .await
    {
        tracing::error!("schedule_retry {id}: {e}");
    }
}

pub fn build_insert_batch<'a>(batch: &'a [&'a NewPush]) -> QueryBuilder<'a, sqlx::Sqlite> {
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

    builder
}

pub async fn insert_batch(batch: &[&NewPush], pool: &SqlitePool) {
    let mut builder = build_insert_batch(batch);
    if let Err(e) = builder.build().execute(pool).await {
        tracing::error!("insert_batch: {e}");
    }
}

pub async fn purge_old_pushes(pool: &SqlitePool) {
    if let Err(e) = sqlx::query(PURGE_OLD_PUSHES_SQL).execute(pool).await {
        tracing::error!("purge_old_pushes: {e}");
    }
}
