use crate::queries::PushResult;
use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use hyper::client::conn::http2::Builder;
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
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
    H2Handshake(hyper::Error),
    Timeout(&'static str),
    InvalidDnsName,
}

impl fmt::Display for AndroidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUri(e) => write!(f, "invalid android_address: {e}"),
            Self::TcpConnect(e) => write!(f, "tcp connect: {e}"),
            Self::TlsHandshake(e) => write!(f, "tls handshake: {e}"),
            Self::H2Handshake(e) => write!(f, "http2 handshake: {e}"),
            Self::Timeout(phase) => write!(f, "{phase} timed out"),
            Self::InvalidDnsName => write!(f, "invalid hostname"),
        }
    }
}

impl std::error::Error for AndroidError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidUri(e) => Some(e),
            Self::TcpConnect(e) => Some(e),
            Self::TlsHandshake(e) => Some(e),
            Self::H2Handshake(e) => Some(e),
            _ => None,
        }
    }
}

pub struct AndroidConnection {
    pub sender: hyper::client::conn::http2::SendRequest<Full<Bytes>>,
    pub api_key: String,
}

impl AndroidConnection {
    pub async fn connect(address: &str, api_key: &str) -> Result<Self, AndroidError> {
        let address_uri: hyper::Uri = address.parse().map_err(AndroidError::InvalidUri)?;
        let host = address_uri.host().unwrap_or("localhost").to_owned();
        let port = address_uri.port_u16().unwrap_or(443);
        let use_tls = address_uri.scheme_str() == Some("https");

        let tcp = tcp_connect(&host, port).await?;

        let io: BoxedIo = if use_tls {
            let tls = tls_connect(tcp, &host).await?;
            TokioIo::new(Box::new(tls) as Box<dyn StreamBox>)
        } else {
            TokioIo::new(Box::new(tcp) as Box<dyn StreamBox>)
        };

        let (sender, http2_connection) = tokio::time::timeout(
            Duration::from_secs(10),
            Builder::new(TokioExecutor::new()).handshake(io),
        )
        .await
        .map_err(|_| AndroidError::Timeout("h2 handshake"))?
        .map_err(AndroidError::H2Handshake)?;

        tokio::spawn(async move {
            if let Err(error) = http2_connection.await {
                tracing::error!("android h2 connection lost: {error}");
            }
        });

        Ok(Self {
            sender,
            api_key: api_key.to_owned(),
        })
    }
}

async fn tcp_connect(host: &str, port: u16) -> Result<TcpStream, AndroidError> {
    let stream = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect((host, port)))
        .await
        .map_err(|_| AndroidError::Timeout("tcp connect"))?
        .map_err(AndroidError::TcpConnect)?;

    if let Err(e) = stream.set_nodelay(true) {
        tracing::error!("android tcp set_nodelay: {e}");
    }

    Ok(stream)
}

async fn tls_connect(tcp: TcpStream, host: &str) -> Result<TlsStream<TcpStream>, AndroidError> {
    let mut root_store = tokio_rustls::rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = tokio_rustls::rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let name = ServerName::try_from(host.to_owned()).map_err(|_| AndroidError::InvalidDnsName)?;

    let tls = tokio::time::timeout(Duration::from_secs(10), connector.connect(name, tcp))
        .await
        .map_err(|_| AndroidError::Timeout("tls handshake"))?
        .map_err(AndroidError::TlsHandshake)?;

    Ok(tls)
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
        Ok(b) => Bytes::from(b),
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
        Ok(req) => req,
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
