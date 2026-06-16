# Durability

## Why SQLite WAL
SQLite in WAL mode lets us read and write concurrently without blocking. Writers append to a WAL file; readers keep going. Handles ~50K writes/sec on modern SSDs.

## Ingestion
* Handler receives POST /pushes with JSON body
* Handler creates Push, sends PushCmd (push + oneshot ack) via tokio mpsc channel
* BatchWriter accumulates PushCmds for up to 5ms or 100 items
* BatchWriter issues multi-row INSERT via sqlx
* After INSERT commits, BatchWriter fires all oneshot acks
* Handler receives ack, returns 200
