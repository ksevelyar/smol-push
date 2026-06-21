CREATE TABLE IF NOT EXISTS pushes (
    id            VARCHAR PRIMARY KEY,
    platform      INTEGER NOT NULL,
    type          VARCHAR NOT NULL,
    text          VARCHAR NOT NULL,
    token         VARCHAR NOT NULL DEFAULT '',
    title         VARCHAR NOT NULL DEFAULT '',
    inserted_at   VARCHAR NOT NULL DEFAULT to_char(NOW(), 'YYYY-MM-DD HH24:MI:SS'),
    retry_count   INTEGER NOT NULL DEFAULT 0,
    next_retry_at BIGINT,
    status        INTEGER NOT NULL DEFAULT 0
);
