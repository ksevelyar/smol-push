# Contract

## API
* endpoint: `POST /pushes`
* request
* responses: [200, 401, 429] with no body
* HTTP 200 indicates the push has been durably accepted by `smol-push`, not necessarily delivered to the device. 

## Delivery Guarantees
* Pushes survive service restarts.
* End-to-end delivery processing is observable via Prometheus metrics.

## Connection Pools
* The environment variable `MAX_CONNECTIONS_PER_PROVIDER` sets the maximum size of the HTTP/2 connection pool per provider (APNS / FCM).
* Each HTTP/2 connection is limited to `MAX_PUSHES_PER_CONNECTION_PER_SECOND` pushes per second.  
* The connection pool dynamically balances load across available connections.
* The service enforces a client-side rate-limiting algorithm per connection to ensure provider rate limits are never breached, preventing connection throttling or bans.

## Retry Policy
* `MAX_RETRY_ATTEMPTS` sets the maximum number of retry attempts per push.  
* Retries use exponential backoff.
* Retries are only attempted for transient errors (e.g., HTTP 5xx, network timeouts, provider rate limits).
* Fatal errors (e.g., HTTP 4xx, Invalid Registration Token, Unregistered device) immediately mark the push as dead without consuming retry attempts.

## Backpressure
* The system applies backpressure when internal queues are full.
* Under overload, new pushes may be rejected with HTTP 429.

## Metrics (Prometheus)
* `smol_push_queue_depth{platform="APNS|FCM"}`: Gauge of currently pending pushes waiting to be sent or retried.
* `smol_push_delivery_total{platform="APNS|FCM", outcome="success|failure", reason="string"}`: Counter of push delivery outcomes.
* `smol_push_rejected_backpressure_total`: Counter of new pushes skipped/rejected with HTTP 429 due to internal queue limits.
* `smol_http2_connections_active{vendor="APNS|FCM"}`: Gauge of currently live HTTP/2 connections per vendor, determined via heartbeat pings.

## Environment Variables
| Variable | Description | Default |
| :--- | :--- | :--- |
| `MAX_CONNECTIONS_PER_PROVIDER` | Maximum size of the HTTP/2 connection pool per provider (APNS / FCM). | `25` |
| `MAX_PUSHES_PER_CONNECTION_PER_SECOND` | Maximum pushes per second allowed per HTTP/2 connection to avoid provider throttling. | `100` |
| `MAX_RETRY_ATTEMPTS` | Maximum number of retry attempts per push for transient errors. | `3` |
| `MAX_QUEUED_PUSHES` | Maximum number of pending pushes held in the system before HTTP 429 is returned to new requests. | `10000` |
| `ANDROID_ADDRESS` | URL for the Android (FCM) HTTP/2 endpoint. Use `http://` for cleartext h2c (dev/testing) or `https://` for production. | `http://127.0.0.1:9099` |
| `RETRY_BASE_DELAY_MS` | Initial delay in milliseconds before the first retry attempt. | `1000` |
| `RETRY_MAX_DELAY_MS` | Maximum cap in milliseconds for the exponential backoff delay. | `60000` |
