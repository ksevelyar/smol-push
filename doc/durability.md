# Durability

## Why [SQLite Write-Ahead Logging](https://sqlite.org/wal.html)
SQLite in WAL mode lets read and write concurrently.

## Ingestion
* Handler receives POST /pushes with JSON body
* Handler creates Push, sends PushCmd (push + oneshot ack) via tokio mpsc channel
* BatchWriter accumulates PushCmds for up to 5ms or 100 items
* BatchWriter issues multi-row INSERT via sqlx
* After INSERT commits, BatchWriter fires all oneshot acks
* Handler receives ack, returns 200
