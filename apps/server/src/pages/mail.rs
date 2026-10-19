//! Sends status page sign-in links through the instance SMTP server, after the code is
//! committed. A failed send is logged; the visitor asks again.

use std::time::Duration;

use lettre::message::header::ContentType;
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
        if let Err(error) = deliver(&state, &to, subject, body).await {
            tracing::warn!(%error, "a status page sign-in link was not sent");
        }
    });
}

async fn deliver(state: &AppState, to: &str, subject: String, body: String) -> anyhow::Result<()> {
    let smtp = {
        let mut conn = state.db.reader.acquire().await?;
        instance::smtp(&mut conn, &state.secrets).await?
    }
    .ok_or_else(|| anyhow::anyhow!("the instance has no SMTP server"))?;
    let message = Message::builder()
        .from(smtp.from.parse()?)
        .to(to.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body)?;
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
