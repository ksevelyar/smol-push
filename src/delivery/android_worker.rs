use crate::delivery::DeliveryConfig;
use crate::queries::{self, PushResult};
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::Full;
use hyper::Request;
use hyper::client::conn::http2::Builder;
use hyper_util::rt::{TokioExecutor, TokioIo};
use sqlx::SqlitePool;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::ServerName;

trait StreamBox: AsyncRead + AsyncWrite + Unpin + Send {}
impl StreamBox for TcpStream {}
impl StreamBox for TlsStream<TcpStream> {}

type BoxedIo = TokioIo<Box<dyn StreamBox>>;

#[derive(Debug)]
pub enum AndroidError {
    InvalidUri(hyper::http::uri::InvalidUri),
    TcpConnect(std::io::Error),
    TlsHandshake(std::io::Error),
    Http2Handshake(hyper::Error),
    Timeout(&'static str),
    InvalidDnsName,
}

impl fmt::Display for AndroidError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUri(error) => write!(formatter, "invalid android_address: {error}"),
            Self::TcpConnect(error) => write!(formatter, "tcp connect: {error}"),
            Self::TlsHandshake(error) => write!(formatter, "tls handshake: {error}"),
            Self::Http2Handshake(error) => write!(formatter, "http2 handshake: {error}"),
            Self::Timeout(phase) => write!(formatter, "{phase} timed out"),
            Self::InvalidDnsName => write!(formatter, "invalid hostname"),
        }
    }
}

impl std::error::Error for AndroidError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidUri(error) => Some(error),
            Self::TcpConnect(error) => Some(error),
            Self::TlsHandshake(error) => Some(error),
            Self::Http2Handshake(error) => Some(error),
            _ => None,
        }
    }
}

pub struct AndroidConnection {
    pub sender: hyper::client::conn::http2::SendRequest<Full<Bytes>>,
    pub api_key: String,
    pub connection_dropped: watch::Receiver<Option<String>>,
}

impl AndroidConnection {
    pub async fn connect(address: &str, api_key: &str) -> Result<Self, AndroidError> {
        let address_uri: hyper::Uri = address.parse().map_err(AndroidError::InvalidUri)?;
        let host = address_uri.host().unwrap_or("localhost").to_owned();
        let port = address_uri.port_u16().unwrap_or(443);
        let use_tls = address_uri.scheme_str() == Some("https");

        let tcp_stream = tcp_connect(&host, port).await?;

        let transport: BoxedIo = if use_tls {
            let tls_stream = tls_connect(tcp_stream, &host).await?;
            TokioIo::new(Box::new(tls_stream) as Box<dyn StreamBox>)
        } else {
            TokioIo::new(Box::new(tcp_stream) as Box<dyn StreamBox>)
        };

        let (sender, http2_connection) = tokio::time::timeout(
            Duration::from_secs(10),
            Builder::new(TokioExecutor::new())
                .timer(hyper_util::rt::TokioTimer::new())
                .keep_alive_interval(Duration::from_secs(10))
                .keep_alive_while_idle(true)
                .handshake(transport),
        )
        .await
        .map_err(|_| AndroidError::Timeout("http2 handshake"))?
        .map_err(AndroidError::Http2Handshake)?;

        let (watch_sender, watch_receiver) = watch::channel(None);

        tokio::spawn(async move {
            if let Err(error) = http2_connection.await {
                tracing::error!("android http2 connection lost: {error}");
                let _ = watch_sender.send(Some(error.to_string()));
            }
        });

        Ok(Self {
            sender,
            api_key: api_key.to_owned(),
            connection_dropped: watch_receiver,
        })
    }
}

async fn tcp_connect(host: &str, port: u16) -> Result<TcpStream, AndroidError> {
    let stream = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect((host, port)))
        .await
        .map_err(|_| AndroidError::Timeout("tcp connect"))?
        .map_err(AndroidError::TcpConnect)?;

    if let Err(error) = stream.set_nodelay(true) {
        tracing::error!("android tcp set_nodelay: {error}");
    }

    Ok(stream)
}

async fn tls_connect(
    tcp_stream: TcpStream,
    host: &str,
) -> Result<TlsStream<TcpStream>, AndroidError> {
    let mut root_store = tokio_rustls::rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls_config = tokio_rustls::rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(tls_config));
    let server_name =
        ServerName::try_from(host.to_owned()).map_err(|_| AndroidError::InvalidDnsName)?;

    let tls_stream = tokio::time::timeout(
        Duration::from_secs(10),
        connector.connect(server_name, tcp_stream),
    )
    .await
    .map_err(|_| AndroidError::Timeout("tls handshake"))?
    .map_err(AndroidError::TlsHandshake)?;

    Ok(tls_stream)
}

pub async fn send_notification(
    sender: &mut hyper::client::conn::http2::SendRequest<Full<Bytes>>,
    api_key: &str,
    token: &str,
    title: &str,
    text: &str,
) -> PushResult {
    let message = serde_json::json!({
        "message": {
            "token": token,
            "notification": {
                "title": title,
                "body": text,
            },
        },
    });

    let body_bytes = match serde_json::to_vec(&message) {
        Ok(bytes) => Bytes::from(bytes),
        Err(_) => return PushResult::RecoverableError,
    };
    let body = Full::new(body_bytes);
    let auth = format!("Bearer {api_key}");

    let request = match Request::builder()
        .method("POST")
        .uri("/")
        .header("content-type", "application/json")
        .header("authorization", &auth)
        .body(body)
    {
        Ok(request) => request,
        Err(_) => return PushResult::RecoverableError,
    };

    let response = match sender.send_request(request).await {
        Ok(response) => response,
        Err(_) => return PushResult::RecoverableError,
    };

    let status = response.status();
    match status {
        status if status.is_success() => PushResult::Delivered,
        hyper::StatusCode::TOO_MANY_REQUESTS => PushResult::RecoverableError,
        status if status.is_client_error() => PushResult::Fatal,
        _ => PushResult::RecoverableError,
    }
}

pub fn spawn(pool: SqlitePool, config: DeliveryConfig, worker_id: usize) {
    tokio::spawn(async move {
        worker_loop(pool, &config, worker_id).await;
    });
}

async fn worker_loop(pool: SqlitePool, config: &DeliveryConfig, worker_id: usize) {
    let mut connection: Option<AndroidConnection> = None;

    loop {
        if connection.is_none() {
            connection = loop {
                match AndroidConnection::connect(&config.android_address, &config.android_api_key)
                    .await
                {
                    Ok(new_connection) => break Some(new_connection),
                    Err(error) => {
                        tracing::error!(
                            "android worker {worker_id} connect: {error}, retry in 5 seconds"
                        );
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            };
        }

        let established_connection = connection.as_mut().unwrap();

        let pushes = queries::select_pending(&pool, queries::Platform::Android, 1000).await;
        if pushes.is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
            continue;
        }

        let mut delivered = Vec::with_capacity(pushes.len());
        let mut dead = Vec::with_capacity(pushes.len());

        for (id, outcome, retry_count) in
            futures_util::stream::iter(pushes.into_iter().map(|push| {
                let mut sender = established_connection.sender.clone();
                let api_key = established_connection.api_key.clone();
                async move {
                    let outcome = send_notification(
                        &mut sender,
                        &api_key,
                        &push.token,
                        &push.title,
                        &push.text,
                    )
                    .await;
                    (push.id, outcome, push.retry_count)
                }
            }))
            .buffer_unordered(config.max_concurrent_streams)
            .collect::<Vec<_>>()
            .await
            .into_iter()
        {
            match outcome {
                PushResult::Delivered => delivered.push(id),
                PushResult::Fatal => dead.push(id),
                PushResult::RecoverableError => {
                    if retry_count >= config.max_retry_attempts {
                        dead.push(id);
                    } else {
                        let delay = retry_delay(
                            retry_count,
                            config.retry_base_delay_milliseconds,
                            config.retry_max_delay_milliseconds,
                        );
                        let next_at = std::time::SystemTime::now() + delay;
                        let ts = next_at
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .unwrap()
                            .as_secs() as i64;
                        queries::schedule_retry(&pool, &id, ts).await;
                    }
                }
            }
        }

        queries::bulk_mark_status(&pool, &delivered, queries::PushStatus::Delivered).await;
        queries::bulk_mark_status(&pool, &dead, queries::PushStatus::Dead).await;
    }
}

fn retry_delay(retry_count: u8, base_milliseconds: u64, maximum_milliseconds: u64) -> Duration {
    Duration::from_millis(
        base_milliseconds * 2u64.pow(retry_count.into()).min(maximum_milliseconds),
    )
}
