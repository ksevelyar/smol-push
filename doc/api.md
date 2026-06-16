# API

## POST /pushes

Accepts a push notification for delivery. Returns 200 only after durably storing to SQLite.

### Request

```
POST /pushes
Content-Type: application/json
api-key: <key>
```

```json
{
  "platform": "apple",
  "type": "info",
  "text": "Hello"
}
```

| Field      | Type     | Description                     |
|------------|----------|---------------------------------|
| `platform` | `string` | `"apple"` or `"android"`        |
| `type`     | `string` | Notification type               |
| `text`     | `string` | Notification body text          |

`platform` is stored as INTEGER (0/1) in SQLite, mapped to a Rust enum at the application layer.

### Responses

| Status | Condition                      | Body |
|--------|--------------------------------|------|
| 200    | Push durably accepted          | none |
| 401    | Missing or invalid `api-key`   | none |
| 429    | Queue full (backpressure)      | none |
| 422    | Malformed JSON                 | none |

### Examples

```
> POST /pushes
> content-type: application/json
> api-key: s3cret
> {"platform":"apple","type":"info","text":"Hello"}
< 200

> POST /pushes
> content-type: application/json
> api-key: wrong-key
> {"platform":"apple","type":"info","text":"Hello"}
< 401

> POST /pushes
> content-type: application/json
> api-key: s3cret
> {}
< 422

> POST /pushes
> content-type: application/json
> api-key: s3cret
> {"platform":"android","type":"alert","text":"Full"}
< 429
```

### Data Model

| Field          | Type      | Description                       |
|----------------|-----------|-----------------------------------|
| id             | uuid (v4) | Server-generated identifier       |
| platform       | INTEGER   | `0` (Apple) or `1` (Android)      |
| type           | TEXT      | Notification type                 |
| text           | TEXT      | Notification body                 |
| inserted_at    | TEXT      | ISO-8601, set on INSERT           |
| retry_count    | INTEGER   | Default 0                         |
| next_retry_at  | TEXT      | Null = ready for delivery         |
| status         | TEXT      | `"pending"`, `"dead"`, `"delivered"` |
