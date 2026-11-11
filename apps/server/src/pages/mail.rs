//! Sends status page mail through the instance SMTP server: sign-in links and subscription
//! confirmations after their code is committed, where a failed send is logged and the visitor
//! asks again, and subscriber notifications, which the delivery queue retries.

use std::time::Duration;

use lettre::message::header::{ContentType, HeaderName, HeaderValue};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::instance;
use crate::state::AppState;

const TIMEOUT: Duration = Duration::from_secs(30);

/// Sends `subject` and `body` to `to` in the background.
pub(super) fn send(state: &AppState, to: String, subject: String, body: String) {
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = deliver(&state, &to, subject, body, None).await {
            tracing::warn!(%error, "a status page mail was not sent");
        }
    });
}

/// Sends a message now. With `unsubscribe` it carries `List-Unsubscribe` naming that URL and
/// `List-Unsubscribe-Post`, so mail clients offer one-click unsubscribing (RFC 8058).
pub(super) async fn deliver(
    state: &AppState,
    to: &str,
    subject: String,
    body: String,
    unsubscribe: Option<&str>,
) -> anyhow::Result<()> {
    let smtp = {
        let mut conn = state.db.reader.acquire().await?;
        instance::smtp(&mut conn, &state.secrets).await?
    }
    .ok_or_else(|| anyhow::anyhow!("the instance has no SMTP server"))?;
    let mut builder = Message::builder()
        .from(smtp.from.parse()?)
        .to(to.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN);
    if let Some(url) = unsubscribe {
        builder = builder
            .raw_header(HeaderValue::new(
                HeaderName::new_from_ascii_str("List-Unsubscribe"),
                format!("<{url}>"),
            ))
            .raw_header(HeaderValue::new(
                HeaderName::new_from_ascii_str("List-Unsubscribe-Post"),
                "List-Unsubscribe=One-Click".to_owned(),
            ));
    }
    let message = builder.body(body)?;
    let tls = || {
        TlsParameters::builder(smtp.host.clone())
            .dangerous_accept_invalid_certs(smtp.ignore_tls_error)
            .dangerous_accept_invalid_hostnames(smtp.ignore_tls_error)
            .build_rustls()
    };
    let tls = if smtp.secure {
        Tls::Wrapper(tls()?)
    } else if smtp.ignore_starttls {
        Tls::None
    } else {
        Tls::Opportunistic(tls()?)
    };
    let mut transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host)
        .port(smtp.port)
        .tls(tls)
        .timeout(Some(TIMEOUT));
    if smtp.username.is_some() || smtp.password.is_some() {
        transport = transport.credentials(Credentials::new(
            smtp.username.clone().unwrap_or_default(),
            smtp.password.clone().unwrap_or_default(),
        ));
    }
    transport.build().send(message).await?;
    Ok(())
}
