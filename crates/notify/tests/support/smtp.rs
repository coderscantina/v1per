//! A minimal SMTP server that stores what it receives, with optional STARTTLS or implicit TLS
//! on a self-signed certificate.

use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use rcgen::{CertificateParams, KeyPair};
use rustls::ServerConfig;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Plain,
    StartTls,
    ImplicitTls,
}

#[derive(Clone, Debug, Default)]
pub struct Mail {
    pub mail_from: String,
    pub rcpt_to: Vec<String>,
    pub data: String,
    pub tls: bool,
    pub auth: Option<(String, String)>,
}

impl Mail {
    /// Unfolded header value, encoded words decoded.
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
        let (_, body) = self.data.split_once("\r\n\r\n").expect("headers end");
        let encoding = self
            .header("Content-Transfer-Encoding")
            .unwrap_or_default()
            .to_ascii_lowercase();
        match encoding.as_str() {
            "base64" => {
                let joined: String = body.split_whitespace().collect();
                String::from_utf8(BASE64.decode(joined).unwrap()).unwrap()
            }
            "quoted-printable" => decode_quoted_printable(body),
            _ => body.to_owned(),
        }
        .replace("\r\n", "\n")
    }
}

pub struct SmtpSink {
    pub port: u16,
    mails: Arc<Mutex<Vec<Mail>>>,
}

impl SmtpSink {
    pub async fn start(mode: Mode) -> Self {
        Self::rejecting_rcpt(mode, None).await
    }

    /// Answers every `RCPT TO` with `reply`, for example `550 5.1.1 unknown user`.
    pub async fn rejecting_rcpt(mode: Mode, reply: Option<&'static str>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mails = Arc::new(Mutex::new(Vec::new()));
        let acceptor = TlsAcceptor::from(self_signed());
        let store = mails.clone();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let (acceptor, store) = (acceptor.clone(), store.clone());
                tokio::spawn(async move {
                    let mut mail = Mail::default();
                    match mode {
                        Mode::Plain => {
                            session(stream, &mut mail, &store, reply, true, false).await;
                        }
                        Mode::ImplicitTls => {
                            let stream = acceptor.accept(stream).await.unwrap();
                            mail.tls = true;
                            session(stream, &mut mail, &store, reply, true, false).await;
                        }
                        Mode::StartTls => {
                            let upgrade =
                                session(stream, &mut mail, &store, reply, true, true).await;
                            if let Some(stream) = upgrade {
                                let stream = acceptor.accept(stream).await.unwrap();
                                mail.tls = true;
                                session(stream, &mut mail, &store, reply, false, false).await;
                            }
                        }
                    }
                });
            }
        });
        Self { port, mails }
    }

    pub fn mails(&self) -> Vec<Mail> {
        self.mails.lock().unwrap().clone()
    }

    pub fn only_mail(&self) -> Mail {
        let mails = self.mails();
        assert_eq!(mails.len(), 1, "expected exactly one mail");
        mails[0].clone()
    }
}

/// Runs commands until QUIT, or returns the stream when the client asks for STARTTLS.
async fn session<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    mail: &mut Mail,
    store: &Mutex<Vec<Mail>>,
    rcpt_reply: Option<&str>,
    greet: bool,
    offer_starttls: bool,
) -> Option<S> {
    let mut stream = BufReader::new(stream);
    if greet {
        write(&mut stream, "220 sink ESMTP\r\n").await;
    }
    let mut line = String::new();
    loop {
        line.clear();
        if stream.read_line(&mut line).await.unwrap_or(0) == 0 {
            return None;
        }
        let command = line.trim_end().to_owned();
        let verb = command
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        match verb.as_str() {
            "EHLO" => {
                let starttls = if offer_starttls {
                    "250-STARTTLS\r\n"
                } else {
                    ""
                };
                let reply = format!("250-sink\r\n{starttls}250-AUTH PLAIN\r\n250 8BITMIME\r\n");
                write(&mut stream, &reply).await;
            }
            "STARTTLS" => {
                write(&mut stream, "220 ready\r\n").await;
                return Some(stream.into_inner());
            }
            "AUTH" => {
                let encoded = command.split_whitespace().nth(2).unwrap_or_default();
                let decoded = String::from_utf8(BASE64.decode(encoded).unwrap()).unwrap();
                let mut parts = decoded.split('\0').skip(1);
                mail.auth = Some((
                    parts.next().unwrap_or_default().to_owned(),
                    parts.next().unwrap_or_default().to_owned(),
                ));
                write(&mut stream, "235 ok\r\n").await;
            }
            "MAIL" => {
                mail.mail_from = address(&command);
                write(&mut stream, "250 ok\r\n").await;
            }
            "RCPT" => match rcpt_reply {
                Some(reply) => write(&mut stream, &format!("{reply}\r\n")).await,
                None => {
                    mail.rcpt_to.push(address(&command));
                    write(&mut stream, "250 ok\r\n").await;
                }
            },
            "DATA" => {
                write(&mut stream, "354 go\r\n").await;
                let mut data = String::new();
                loop {
                    line.clear();
                    stream.read_line(&mut line).await.unwrap();
                    if line == ".\r\n" {
                        break;
                    }
                    data.push_str(line.strip_prefix('.').unwrap_or(&line));
                }
                // The CRLF before the final dot belongs to the terminator.
                mail.data = data.strip_suffix("\r\n").unwrap_or(&data).to_owned();
                store.lock().unwrap().push(mail.clone());
                write(&mut stream, "250 2.0.0 queued\r\n").await;
            }
            "QUIT" => {
                write(&mut stream, "221 bye\r\n").await;
                return None;
            }
            "RSET" | "NOOP" => write(&mut stream, "250 ok\r\n").await,
            _ => write(&mut stream, "500 unknown\r\n").await,
        }
    }
}

async fn write<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut BufReader<S>, reply: &str) {
    stream.get_mut().write_all(reply.as_bytes()).await.unwrap();
    stream.get_mut().flush().await.unwrap();
}

fn address(command: &str) -> String {
    let start = command.find('<').map_or(0, |i| i + 1);
    let end = command.find('>').unwrap_or(command.len());
    command[start..end].to_owned()
}

fn self_signed() -> Arc<ServerConfig> {
    let key = KeyPair::generate().unwrap();
    let cert = CertificateParams::new(vec!["localhost".to_owned()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    Arc::new(
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert.der().clone()], key)
            .unwrap(),
    )
}

/// RFC 2047 encoded words; adjacent words join without the whitespace between them.
fn decode_words(value: &str) -> String {
    let mut out = String::new();
    let mut previous_encoded = false;
    for token in value.split_whitespace() {
        let encoded = token
            .strip_prefix("=?")
            .and_then(|t| t.strip_suffix("?="))
            .and_then(|t| {
                let mut parts = t.splitn(3, '?');
                let (_, kind, text) = (parts.next()?, parts.next()?, parts.next()?);
                match kind {
                    "b" | "B" => String::from_utf8(BASE64.decode(text).ok()?).ok(),
                    "q" | "Q" => Some(decode_quoted_printable(&text.replace('_', " "))),
                    _ => None,
                }
            });
        match encoded {
            Some(text) => {
                if !out.is_empty() && !previous_encoded {
                    out.push(' ');
                }
                out.push_str(&text);
                previous_encoded = true;
            }
            None => {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(token);
                previous_encoded = false;
            }
        }
    }
    out
}

fn decode_quoted_printable(text: &str) -> String {
    let text = text.replace("=\r\n", "");
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'='
            && i + 3 <= bytes.len()
            && let Ok(byte) = u8::from_str_radix(&text[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap()
}
