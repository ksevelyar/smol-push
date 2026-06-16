CREATE TABLE IF NOT EXISTS pushes (
    id            TEXT PRIMARY KEY,
    platform      INTEGER NOT NULL,
    type          TEXT NOT NULL,
    text          TEXT NOT NULL,
    inserted_at   TEXT NOT NULL DEFAULT (datetime('now')),
    retry_count   INTEGER NOT NULL DEFAULT 0,
    next_retry_at TEXT,
    status        TEXT NOT NULL DEFAULT 'pending'
);
