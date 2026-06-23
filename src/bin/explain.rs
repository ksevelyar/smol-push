use smol_push::queries::{self, NewPush, Platform, PushStatus};
use smol_push::utils::TestDatabase;
use sqlx::SqlitePool;
use std::time::Instant;

const ROWS: usize = 100_000;

#[tokio::main]
async fn main() {
    let database = TestDatabase::new().await;
    let pool = database.pool().clone();
    fill_deterministic(&pool).await;

    sqlx::query("VACUUM").execute(&pool).await.unwrap();
    sqlx::query("ANALYZE").execute(&pool).await.unwrap();

    let sample_ids: Vec<String> = (0..100).map(|i| format!("push-{i}")).collect();
    let sample_pushes: Vec<NewPush> = (0..100)
        .map(|i| NewPush {
            id: format!("new-{i}"),
            platform: Platform::Android,
            r#type: "info".into(),
            text: "hello".into(),
            token: format!("token-{i}"),
            title: String::new(),
        })
        .collect();
    let sample_refs: Vec<&NewPush> = sample_pushes.iter().collect();

    println!("═══ Query Plan Analysis ({ROWS} rows, VACUUM'd + ANALYZE'd) ═══");

    run("fetch_and_lock (platform=Android, limit=1000)", || async {
        let plan = queries::explain(&pool, queries::FETCH_AND_LOCK_SQL).await;
        let start = Instant::now();
        let rows = queries::fetch_and_lock(&pool, Platform::Android, 1000).await;
        (plan, rows.len(), start.elapsed())
    })
    .await;

    run(
        "fetch_and_lock (platform=Apple, limit=1000) — ZERO MATCH",
        || async {
            let plan = queries::explain(&pool, queries::FETCH_AND_LOCK_SQL).await;
            let start = Instant::now();
            let rows = queries::fetch_and_lock(&pool, Platform::Apple, 1000).await;
            (plan, rows.len(), start.elapsed())
        },
    )
    .await;

    run("select_pending (platform=Android, limit=1000)", || async {
        let plan = queries::explain(&pool, queries::SELECT_PENDING_SQL).await;
        let start = Instant::now();
        let rows = queries::select_pending(&pool, Platform::Android, 1000).await;
        (plan, rows.len(), start.elapsed())
    })
    .await;

    run(
        "select_pending (platform=Apple, limit=1000) — ZERO MATCH",
        || async {
            let plan = queries::explain(&pool, queries::SELECT_PENDING_SQL).await;
            let start = Instant::now();
            let rows = queries::select_pending(&pool, Platform::Apple, 1000).await;
            (plan, rows.len(), start.elapsed())
        },
    )
    .await;

    run("reset_stale", || async {
        let plan = queries::explain(&pool, queries::RESET_STALE_SQL).await;
        let start = Instant::now();
        queries::reset_stale(&pool).await;
        (plan, 0, start.elapsed())
    })
    .await;

    run("bulk_mark_status (100 ids, status=Delivered)", || async {
        let builder = queries::build_bulk_mark_status(&sample_ids, PushStatus::Delivered).unwrap();
        let plan = queries::explain(&pool, builder.sql()).await;
        let start = Instant::now();
        queries::bulk_mark_status(&pool, &sample_ids, PushStatus::Delivered).await;
        (plan, sample_ids.len(), start.elapsed())
    })
    .await;

    run("bulk_mark_status (1 id, status=Delivered)", || async {
        let builder =
            queries::build_bulk_mark_status(&sample_ids[..1], PushStatus::Delivered).unwrap();
        let plan = queries::explain(&pool, builder.sql()).await;
        let start = Instant::now();
        queries::bulk_mark_status(&pool, &sample_ids[..1], PushStatus::Delivered).await;
        (plan, 1, start.elapsed())
    })
    .await;

    run("schedule_retry (existing id push-0)", || async {
        let plan = queries::explain(&pool, queries::SCHEDULE_RETRY_SQL).await;
        let start = Instant::now();
        queries::schedule_retry(&pool, "push-0", 9999999999).await;
        (plan, 0, start.elapsed())
    })
    .await;

    run("schedule_retry (non-existent id missing-0)", || async {
        let plan = queries::explain(&pool, queries::SCHEDULE_RETRY_SQL).await;
        let start = Instant::now();
        queries::schedule_retry(&pool, "missing-0", 9999999999).await;
        (plan, 0, start.elapsed())
    })
    .await;

    run("insert_batch (100 rows)", || async {
        let builder = queries::build_insert_batch(&sample_refs);
        let plan = queries::explain(&pool, builder.sql()).await;
        let start = Instant::now();
        queries::insert_batch(&sample_refs, &pool).await;
        (plan, sample_refs.len(), start.elapsed())
    })
    .await;

    run("insert_batch (1 row)", || async {
        let builder = queries::build_insert_batch(&sample_refs[..1]);
        let plan = queries::explain(&pool, builder.sql()).await;
        let start = Instant::now();
        queries::insert_batch(&sample_refs[..1], &pool).await;
        (plan, 1, start.elapsed())
    })
    .await;

    run("purge_old_pushes", || async {
        let plan = queries::explain(&pool, queries::PURGE_OLD_PUSHES_SQL).await;
        let start = Instant::now();
        queries::purge_old_pushes(&pool).await;
        (plan, 0, start.elapsed())
    })
    .await;
}

async fn fill_deterministic(pool: &SqlitePool) {
    let pushes: Vec<NewPush> = (0..ROWS)
        .map(|i| NewPush {
            id: format!("push-{i}"),
            platform: if i % 2 == 0 {
                Platform::Android
            } else {
                Platform::Apple
            },
            r#type: "info".into(),
            text: "bench".into(),
            token: format!("token-{i}"),
            title: String::new(),
        })
        .collect();

    for chunk in pushes.chunks(5000) {
        let refs: Vec<&NewPush> = chunk.iter().collect();
        queries::insert_batch(&refs, pool).await;
    }
}

async fn run<F, Fut>(label: &str, f: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = (Vec<queries::ExplainRow>, usize, std::time::Duration)>,
{
    let (plan, row_count, elapsed) = f().await;

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  {label}");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    for row in &plan {
        println!(
            "  id={} parent={} detail={}",
            row.id, row.parent, row.detail
        );
    }
    if plan.is_empty() {
        println!("  (no plan)");
    }
    println!();
    println!(
        "  RESULT:  {row_count} rows,  {}.{:03}ms",
        elapsed.as_millis(),
        elapsed.subsec_millis(),
    );
}
