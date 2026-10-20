//! Local servers and certificates for the integration tests. Nothing here reaches the internet.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair, date_time_ymd,
};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use v1per_checks::{CheckContext, CheckDefinition};

pub struct Request {
    pub method: String,
    pub path: String,
    /// The raw request head, lowercased.
    pub head: String,
}

pub struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: String,
    delay: Duration,
}

impl Reply {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
            delay: Duration::ZERO,
        }
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

/// Serves HTTP/1.1 on 127.0.0.1 and returns the base URL (`https://localhost:port` with TLS).
pub async fn serve<F>(tls: Option<Arc<ServerConfig>>, handler: F) -> String
where
    F: Fn(&Request) -> Reply + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handler = Arc::new(handler);
    let acceptor = tls.clone().map(TlsAcceptor::from);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let handler = handler.clone();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                match acceptor {
                    Some(acceptor) => {
                        if let Ok(stream) = acceptor.accept(stream).await {
                            respond(stream, handler.as_ref()).await;
                        }
                    }
                    None => respond(stream, handler.as_ref()).await,
                }
            });
        }
    });
    match tls {
        Some(_) => format!("https://localhost:{port}"),
        None => format!("http://127.0.0.1:{port}"),
    }
}

async fn respond<S, F>(stream: S, handler: &F)
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: Fn(&Request) -> Reply,
{
    let mut stream = BufReader::new(stream);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        if stream.read_line(&mut line).await.unwrap_or(0) == 0 {
            return;
        }
        if line == "\r\n" {
            break;
        }
        head.push_str(&line);
    }
    let mut parts = head.split_whitespace();
    let request = Request {
        method: parts.next().unwrap_or_default().to_owned(),
        path: parts.next().unwrap_or_default().to_owned(),
        head: head.to_lowercase(),
    };
    let reply = handler(&request);
    tokio::time::sleep(reply.delay).await;
    let mut response = format!(
        "HTTP/1.1 {} X\r\ncontent-length: {}\r\nconnection: close\r\n",
        reply.status,
        reply.body.len()
    );
    for (name, value) in &reply.headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(&reply.body);
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// A test CA and server configs for localhost: valid, expired, self-signed.
pub struct Pki {
    pub ca: Vec<u8>,
    pub valid: Arc<ServerConfig>,
    pub expired: Arc<ServerConfig>,
    pub self_signed: Arc<ServerConfig>,
}

impl Pki {
    pub fn new() -> Self {
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "V1Per test CA");
        let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();

        let leaf = |configure: &dyn Fn(&mut CertificateParams)| {
            let key = KeyPair::generate().unwrap();
            let mut params = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
            params
                .distinguished_name
                .push(DnType::CommonName, "localhost");
            configure(&mut params);
            (params, key)
        };
        let (params, key) = leaf(&|_| {});
        let valid = server_config(params.signed_by(&key, &ca).unwrap().der().clone(), &key);
        let (params, key) = leaf(&|params| {
            params.not_before = date_time_ymd(2020, 1, 1);
            params.not_after = date_time_ymd(2021, 1, 1);
        });
        let expired = server_config(params.signed_by(&key, &ca).unwrap().der().clone(), &key);
        let (params, key) = leaf(&|_| {});
        let self_signed = server_config(params.self_signed(&key).unwrap().der().clone(), &key);

        Self {
            ca: ca.der().to_vec(),
            valid,
            expired,
            self_signed,
        }
    }

    /// A context that trusts this CA.
    pub fn context(&self) -> CheckContext {
        CheckContext::with_extra_root_certs([self.ca.clone()]).unwrap()
    }
}

fn server_config(cert: CertificateDer<'static>, key: &KeyPair) -> Arc<ServerConfig> {
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    Arc::new(
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .unwrap(),
    )
}

/// Builds a definition from Kuma-shaped JSON, the way the importer and protocol hand it over.
pub fn definition(json: serde_json::Value) -> CheckDefinition {
    serde_json::from_value(json).unwrap()
}
