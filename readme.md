# SmolPush
Async push notification microservice focused on throughput, low resources, and observability.

## Ingestion
POST /pushes returns 200 only after durable SQLite write. Pushes are batched in memory (up to 100 or 5ms) and flushed via multi-row INSERT into SQLite WAL.

## Delivery
Background worker polls pending pushes and sends them via HTTP/2 multiplexing over a single connection to the provider. Retries use exponential backoff.

## Benchmark
```
cargo bench
```

```
end_to_end     fastest       │ slowest       │ median        │ mean          │ samples │ iters
╰─ throughput                │               │               │               │         │
   ├─ 1000     27.2 ms       │ 50.97 ms      │ 38.12 ms      │ 36.44 ms      │ 10      │ 10
   │           36.76 Kitem/s │ 19.61 Kitem/s │ 26.23 Kitem/s │ 27.43 Kitem/s │         │
   ├─ 10000    141.3 ms      │ 168 ms        │ 149.5 ms      │ 151.1 ms      │ 10      │ 10
   │           70.75 Kitem/s │ 59.5 Kitem/s  │ 66.88 Kitem/s │ 66.17 Kitem/s │         │
   ├─ 20000    223.3 ms      │ 284.4 ms      │ 252.2 ms      │ 250.7 ms      │ 10      │ 10
   │           89.55 Kitem/s │ 70.31 Kitem/s │ 79.29 Kitem/s │ 79.74 Kitem/s │         │
   ╰─ 50000    667.1 ms      │ 819.4 ms      │ 748.9 ms      │ 746.1 ms      │ 10      │ 10
               74.94 Kitem/s │ 61.01 Kitem/s │ 66.75 Kitem/s │ 67.01 Kitem/s │         │
```
