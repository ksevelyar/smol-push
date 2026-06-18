# SmolPush
Async push notification microservice focused on throughput, low resources, and observability.

## Ingestion
POST /pushes returns 200 only after durable SQLite write. Pushes are batched in memory (up to 100 or 5ms) and flushed via multi-row INSERT into SQLite WAL.

## Delivery
Background worker polls pending pushes and sends them via HTTP/2 multiplexing over a single connection to the provider. Retries use exponential backoff.

## Benchmark (10k pushes, 1 TCP connection)
```
cargo test --test benchmark --release -- --nocapture
```

```
=== BENCHMARK ===
pushes:          10000
ingest:          73.400533ms  (  136239/s)
delivery:        205.609027ms  (   48636/s)
total:           279.00956ms  (   35841/s)
```
