//! Kuma `smtp.js` over lettre instead of nodemailer.

use std::error::Error as _;

use lettre::message::dkim::{
    DkimCanonicalization, DkimCanonicalizationType, DkimConfig, DkimSigningAlgorithm,
    DkimSigningKey,
};
use lettre::message::header::{self, ContentType, HeaderName, HeaderValue};
use lettre::message::{Mailbox, Mailboxes, MessageBuilder};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use rsa::RsaPrivateKey;
use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding};
use rsa::pkcs8::DecodePrivateKey;

use crate::config::SmtpConfig;
use crate::error::SendError;
use crate::event::Notice;
use crate::providers::{additional_headers, trimmed};
use crate::{Delivered, SendContext, template};

/// Signed when `smtpDkimheaderFieldNames` is unset; the common part of nodemailer's default.
const DKIM_DEFAULT_HEADERS: &[&str] = &[
    "From",
    "Sender",
    "Reply-To",
    "Subject",
    "Date",
    "Message-ID",
    "To",
    "Cc",
    "MIME-Version",
    "Content-Type",
    "Content-Transfer-Encoding",
];

pub(crate) async fn send(
    config: &SmtpConfig,
    notice: &Notice<'_>,
    ctx: &SendContext,
) -> Result<Delivered, SendError> {
    let host = config
        .host
        .as_deref()
        .ok_or_else(|| SendError::Config("SMTP host is missing".into()))?;
    let headers = additional_headers(config.additional_headers.as_ref())?;
    let mail = compose(config, notice)?;
    let message = message(config, mail, headers)?;
    let response = transport(config, host, ctx)?
        .send(message)
        .await
        .map_err(smtp_error)?;
    let text: Vec<_> = response.message().collect();
    Ok(Delivered {
        detail: format!("{} {}", response.code(), text.join(" ")),
    })
}

struct Mail {
    subject: String,
    body: String,
    html: bool,
}

/// Subject and body (`smtp.js:70-89`).
fn compose(config: &SmtpConfig, notice: &Notice<'_>) -> Result<Mail, SendError> {
    let mut mail = Mail {
        subject: notice.msg.clone(),
        body: match &notice.beat {
            Some(beat) => format!(
                "{}\nTime ({}): {}",
                notice.msg, beat.timezone, beat.local_date_time
            ),
            None => notice.msg.clone(),
        },
        html: false,
    };
    // Certificate and domain mails never use templates; up, down and tests do.
    if notice.beat.is_none() && !notice.msg.ends_with("Testing") {
        return Ok(mail);
    }
    let render = |template| {
        template::render(
            template,
            &notice.msg,
            &notice.monitor_json,
            &notice.heartbeat_json,
        )
    };
    if let Some(subject) = trimmed(config.custom_subject.as_deref()) {
        mail.subject = render(subject)?;
    }
    if let Some(body) = trimmed(config.custom_body.as_deref()) {
        mail.body = render(body)?;
        mail.html = config.html_body;
    }
    Ok(mail)
}

fn message(
    config: &SmtpConfig,
    mail: Mail,
    headers: Vec<(String, String)>,
) -> Result<Message, SendError> {
    let from: Mailbox = config
        .from
        .as_deref()
        .ok_or_else(|| SendError::Config("sender address is missing".into()))?
        .parse()
        .map_err(|error| SendError::Config(format!("invalid sender address: {error}")))?;
    let mut builder = Message::builder().from(from).subject(mail.subject);

    let mut has_recipient = false;
    for (list, add) in [
        (
            &config.to,
            add_to as fn(MessageBuilder, Mailboxes) -> MessageBuilder,
        ),
        (&config.cc, add_cc),
        (&config.bcc, add_bcc),
    ] {
        let Some(list) = list else { continue };
        let mailboxes: Mailboxes = list
            .parse()
            .map_err(|error| SendError::Config(format!("invalid recipient list: {error}")))?;
        has_recipient = true;
        builder = add(builder, mailboxes);
    }
    if !has_recipient {
        return Err(SendError::Config("no recipient (to, cc or bcc)".into()));
    }

    for (name, value) in headers {
        let name = HeaderName::new_from_ascii(name)
            .map_err(|error| SendError::Config(format!("invalid additional header: {error}")))?;
        builder = builder.raw_header(HeaderValue::new(name, value));
    }

    let content_type = if mail.html {
        ContentType::TEXT_HTML
    } else {
        ContentType::TEXT_PLAIN
    };
    let mut message = builder
        .header(content_type)
        .body(mail.body)
        .map_err(|error| SendError::Config(format!("cannot build the message: {error}")))?;

    if let Some(dkim) = dkim(config)? {
        message.sign(&dkim);
    }
    Ok(message)
}

fn add_to(builder: MessageBuilder, mailboxes: Mailboxes) -> MessageBuilder {
    builder.mailbox(header::To::from(mailboxes))
}

fn add_cc(builder: MessageBuilder, mailboxes: Mailboxes) -> MessageBuilder {
    builder.mailbox(header::Cc::from(mailboxes))
}

fn add_bcc(builder: MessageBuilder, mailboxes: Mailboxes) -> MessageBuilder {
    builder.mailbox(header::Bcc::from(mailboxes))
}

/// nodemailer's DKIM options (`smtp.js:39-48`). Always rsa-sha256 with relaxed/relaxed.
fn dkim(config: &SmtpConfig) -> Result<Option<DkimConfig>, SendError> {
    let Some(domain) = &config.dkim_domain else {
        return Ok(None);
    };
    let selector = config
        .dkim_key_selector
        .clone()
        .ok_or_else(|| SendError::Config("DKIM key selector is missing".into()))?;
    let pem = config
        .dkim_private_key
        .as_ref()
        .ok_or_else(|| SendError::Config("DKIM private key is missing".into()))?
        .expose();
    let invalid_key = || SendError::Config("DKIM private key is not an RSA PEM key".into());
    // lettre reads PKCS#1 only; nodemailer also takes PKCS#8.
    let key = if pem.contains("BEGIN PRIVATE KEY") {
        let key = RsaPrivateKey::from_pkcs8_pem(pem).map_err(|_| invalid_key())?;
        let pkcs1 = key
            .to_pkcs1_pem(LineEnding::LF)
            .map_err(|_| invalid_key())?;
        DkimSigningKey::new(&pkcs1, DkimSigningAlgorithm::Rsa)
    } else {
        DkimSigningKey::new(pem, DkimSigningAlgorithm::Rsa)
    }
    .map_err(|_| invalid_key())?;

    let split = |list: &str| -> Vec<String> {
        list.split(':')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_ascii_lowercase)
            .collect()
    };
    let skip = config
        .dkim_skip_fields
        .as_deref()
        .map(split)
        .unwrap_or_default();
    let names = match &config.dkim_header_field_names {
        Some(list) => split(list),
        None => DKIM_DEFAULT_HEADERS
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect(),
    };
    let headers = names
        .into_iter()
        .filter(|name| !skip.contains(name))
        .map(|name| {
            HeaderName::new_from_ascii(name)
                .map_err(|_| SendError::Config("invalid DKIM header field name".into()))
        })
        .collect::<Result<_, _>>()?;

    Ok(Some(DkimConfig::new(
        selector,
        domain.clone(),
        key,
        headers,
        DkimCanonicalization {
            header: DkimCanonicalizationType::Relaxed,
            body: DkimCanonicalizationType::Relaxed,
        },
    )))
}

/// nodemailer's transport options (`smtp.js:14-56`).
fn transport(
    config: &SmtpConfig,
    host: &str,
    ctx: &SendContext,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, SendError> {
    let tls_parameters = || {
        TlsParameters::builder(host.to_owned())
            .dangerous_accept_invalid_certs(config.ignore_tls_error)
            .dangerous_accept_invalid_hostnames(config.ignore_tls_error)
            .build_rustls()
            .map_err(|error| SendError::Config(format!("TLS setup failed: {error}")))
    };
    let tls = if config.secure {
        Tls::Wrapper(tls_parameters()?)
    } else if config.ignore_starttls {
        Tls::None
    } else {
        // nodemailer upgrades with STARTTLS when the server offers it.
        Tls::Opportunistic(tls_parameters()?)
    };
    let port = config.port.unwrap_or(if config.secure { 465 } else { 587 });

    let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
        .port(port)
        .tls(tls)
        .timeout(Some(ctx.timeout));
    if config.username.is_some() || config.password.is_some() {
        builder = builder.credentials(Credentials::new(
            config.username.clone().unwrap_or_default(),
            config
                .password
                .as_ref()
                .map(|p| p.expose().to_owned())
                .unwrap_or_default(),
        ));
    }
    Ok(builder.build())
}

/// 5xx replies and client-side failures are permanent; 4xx, network and TLS are retryable.
fn smtp_error(error: lettre::transport::smtp::Error) -> SendError {
    let permanent = error.is_permanent() || error.is_client();
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    SendError::Smtp { message, permanent }
}
