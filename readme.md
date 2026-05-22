# SmolPush
A lightweight, async push delivery microservice built with Axum and PostgreSQL.

## Testing Philosophy
Rate limiting is enforced through behavior-driven tests using a dedicated test adapter. The adapter counts outbound push attempts and reports them back to the test process. When a configured quota is exceeded, the test must fail by timeout: the adapter should never receive more pushes than the allowed limit within the defined window.

## Telemetry
SmolPush emits Telemetry events focused on HTTP/2 connection state. Because the service uses persistent HTTP/2 connections exclusively, it exposes metrics for active connections per vendor (APNS, FCM, Web Push), allowing operators to verify that exactly three pools per vendor are established and healthy.
