//! Local stand-ins for notification providers: a Discord webhook and a plain SMTP server. They
//! record what they receive and never touch the network.

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// A Discord webhook. `GET` answers the webhook info Kuma checks for an avatar; every other
/// request is recorded and answered with the current status.
pub struct Discord {
    /// The webhook URL, with a token in its path like a real one.
    pub url: String,
    status: Arc<AtomicU16>,
    posts: Arc<Mutex<Vec<Value>>>,
}

impl Discord {
    pub async fn start() -> Self {
        let status = Arc::new(AtomicU16::new(204));
        let posts = Arc::new(Mutex::new(Vec::new()));
        let (answer, store) = (status.clone(), posts.clone());
        let app = Router::new().fallback(move |method: Method, body: Bytes| {
            let (answer, store) = (answer.clone(), store.clone());
            async move {
                if method == Method::GET {
                    return (StatusCode::OK, r#"{"id":"1","avatar":"a1b2c3"}"#.to_owned());
                }
                let status = StatusCode::from_u16(answer.load(Ordering::SeqCst)).unwrap();
                if status.is_success() {
                    store
                        .lock()
                        .unwrap()
                        .push(serde_json::from_slice(&body).unwrap());
                }
                (status, String::new())
            }
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/api/webhooks/42/s3cr3t-webhook-token",
            listener.local_addr().unwrap()
        );
        tokio::spawn(async move { axum::serve(listener, app).await });
        Self { url, status, posts }
    }

    /// The status answered to posts from now on, such as 503 for an outage.
    pub fn answer(&self, status: u16) {
        self.status.store(status, Ordering::SeqCst);
    }

    /// The JSON bodies accepted so far.
    pub fn posts(&self) -> Vec<Value> {
        self.posts.lock().unwrap().clone()
    }
}

#[derive(Clone, Debug)]
pub struct Mail {
    pub from: String,
    pub to: Vec<String>,
    pub data: String,
    /// The `AUTH PLAIN` credentials as `user:password`.
    pub auth: Option<String>,
}

impl Mail {
    /// A header's value, unfolded, with encoded words decoded.
    pub fn header(&self, name: &str) -> Option<String> {
        let (head, _) = self.data.split_once("\r\n\r\n")?;
        let unfolded = head.replace("\r\n ", " ").replace("\r\n\t", " ");
        unfolded.split("\r\n").find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| decode_words(value.trim()))
        })
    }

    /// The decoded body of a single-part message.
    pub fn body(&self) -> String {
        let (_, body) = self.data.split_once("\r\n\r\n").unwrap();
        let encoding = self
            .header("Content-Transfer-Encoding")
            .unwrap_or_default()
            .to_ascii_lowercase();
        match encoding.as_str() {
            "base64" => {
                let joined: String = body.split_whitespace().collect();
                String::from_utf8(BASE64.decode(joined).unwrap()).unwrap()
            }
            "quoted-printable" => quoted_printable(body),
            _ => body.to_owned(),
        }
        .replace("\r\n", "\n")
    }
}

/// `=?utf-8?b?...?=` words, as mail libraries encode non-ASCII subjects. Whitespace between
/// two encoded words is dropped, as RFC 2047 says.
fn decode_words(value: &str) -> String {
    let mut out = String::new();
    let mut previous_encoded = false;
    for word in value.split_whitespace() {
        let inner = word.strip_prefix("=?").and_then(|w| w.strip_suffix("?="));
        let decoded = match inner.map(|w| w.splitn(3, '?').collect::<Vec<_>>()) {
            Some(parts) if parts.len() == 3 && parts[1].eq_ignore_ascii_case("b") => {
                Some(String::from_utf8(BASE64.decode(parts[2]).unwrap()).unwrap())
            }
            _ => None,
        };
        if !out.is_empty() && !(previous_encoded && decoded.is_some()) {
            out.push(' ');
        }
        previous_encoded = decoded.is_some();
        out.push_str(&decoded.unwrap_or_else(|| word.to_owned()));
    }
    out
}

fn quoted_printable(text: &str) -> String {
    let joined = text.replace("=\r\n", "");
    let bytes = joined.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
            out.push(u8::from_str_radix(hex, 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

/// An SMTP server without TLS or authentication that keeps every message.
pub struct SmtpSink {
    pub port: u16,
    mails: Arc<Mutex<Vec<Mail>>>,
}

impl SmtpSink {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mails = Arc::new(Mutex::new(Vec::new()));
        let store = mails.clone();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let store = store.clone();
                tokio::spawn(async move {
                    let (read, mut write) = socket.into_split();
                    let mut lines = BufReader::new(read).lines();
                    let mut mail = Mail {
                        from: String::new(),
                        to: Vec::new(),
                        data: String::new(),
                        auth: None,
                    };
                    write.write_all(b"220 sink ESMTP\r\n").await.unwrap();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let upper = line.to_ascii_uppercase();
                        let reply: &[u8] = if upper.starts_with("EHLO") || upper.starts_with("HELO")
                        {
                            b"250-sink\r\n250 AUTH PLAIN\r\n"
                        } else if let Some(encoded) = line.strip_prefix("AUTH PLAIN ") {
                            let plain = BASE64.decode(encoded.trim()).unwrap();
                            let parts: Vec<String> = plain
                                .split(|byte| *byte == 0)
                                .map(|part| String::from_utf8_lossy(part).into_owned())
                                .collect();
                            mail.auth = Some(format!("{}:{}", parts[1], parts[2]));
                            b"235 accepted\r\n"
                        } else if let Some(from) = upper.strip_prefix("MAIL FROM:") {
                            mail.from = line[line.len() - from.len()..].trim().to_owned();
                            b"250 OK\r\n"
                        } else if let Some(to) = upper.strip_prefix("RCPT TO:") {
                            mail.to
                                .push(line[line.len() - to.len()..].trim().to_owned());
                            b"250 OK\r\n"
                        } else if upper == "DATA" {
                            write.write_all(b"354 go ahead\r\n").await.unwrap();
                            let mut data = Vec::new();
                            while let Ok(Some(line)) = lines.next_line().await {
                                if line == "." {
                                    break;
                                }
                                data.push(
                                    line.strip_prefix('.').map_or(line.clone(), str::to_owned),
                                );
                            }
                            mail.data = data.join("\r\n");
                            store.lock().unwrap().push(mail.clone());
                            b"250 queued\r\n"
                        } else if upper == "QUIT" {
                            write.write_all(b"221 bye\r\n").await.unwrap();
                            break;
                        } else {
                            b"250 OK\r\n"
                        };
                        write.write_all(reply).await.unwrap();
                    }
                });
            }
        });
        Self { port, mails }
    }

    pub fn mails(&self) -> Vec<Mail> {
        self.mails.lock().unwrap().clone()
    }
}
