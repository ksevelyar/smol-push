# API
- endpoint: `POST /pushes`
- request
- responses: [200, 401, 429] with no body
- HTTP 200 indicates the push has been durably **accepted** by `smol-push`, not necessarily delivered to the device. 

## Data Model
Each push includes:
- Unique identifier
- Target platform (APNS or FCM)
- JSON payload
- Retry metadata
