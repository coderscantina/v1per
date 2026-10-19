//! `instance.*` operations: what this v1per instance is, and the settings its admin changes.
//!
//! `V1PER_INSTANCE_NAME` and `V1PER_PUBLIC_URL` win over the stored settings while set, so a
//! deployment that configures them keeps them. Only `V1PER_PUBLIC_URL` also guards commands
//! and marks cookies `Secure`; a stored public URL is the base of links and nothing else, so
//! a wrong value typed in the app cannot lock anyone out.
//!
//! The timezone and certificate thresholds are Kuma's `serverTimezone` and
//! `tlsExpiryNotifyDays`. The SMTP server sends for email channels without their own; its
//! password is sealed under `instance:smtp:password`. Its test mail goes through the
//! notification outbox like a channel test, on the root node.

use chrono_tz::Tz;
use serde_json::json;
use sqlx::SqliteConnection;
use v1per_protocol::envelope::{Empty, Topic};
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::NodeId;
use v1per_protocol::operations::channels::{ChannelSettings, ChannelTestQueued, SmtpChannel};
use v1per_protocol::operations::instance::{
    InstanceHandlers, InstanceInfo, InstanceOperations, InstanceSettings, InstanceTestSmtpArgs,
    InstanceUpdateSettingsArgs, InstanceUpdateSmtpArgs, SmtpSettings,
};
use v1per_protocol::operations::monitors::SecretField;
use v1per_protocol::registry::Snapshot;

use crate::config::{Config, origin_of};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::identity::secrets::SecretKey;
use crate::identity::validate_name;
use crate::notifications::config::{Kuma, to_kuma};
use crate::notifications::queue_test;
use crate::state::AppState;

/// The name an instance has before anyone names it.
const DEFAULT_NAME: &str = "v1per";
const SMTP_PASSWORD: &str = "instance:smtp:password";
/// Kuma's default `tlsExpiryNotifyDays`, used when the stored list does not parse.
const DEFAULT_TLS_DAYS: [u32; 3] = [7, 14, 21];
const TLS_DAYS_MAX: usize = 10;
const TLS_DAY_MAX: u32 = 365;

/// The settings in effect: configuration first, then the stored values.
pub struct Effective {
    pub name: String,
    pub public_url: Option<String>,
    pub timezone: Tz,
    pub tls_expiry_notify_days: Vec<u32>,
}

#[derive(sqlx::FromRow)]
struct Row {
    name: Option<String>,
    public_url: Option<String>,
    timezone: String,
    tls_expiry_notify_days: String,
    smtp_host: Option<String>,
    smtp_port: Option<i64>,
    smtp_secure: bool,
    smtp_ignore_tls_error: bool,
    smtp_ignore_starttls: bool,
    smtp_username: Option<String>,
    smtp_password: Option<Vec<u8>>,
    smtp_from: Option<String>,
}

async fn row(conn: &mut SqliteConnection) -> Result<Row, sqlx::Error> {
    sqlx::query_as(
        "SELECT name, public_url, timezone, tls_expiry_notify_days, smtp_host, smtp_port,
                smtp_secure, smtp_ignore_tls_error, smtp_ignore_starttls, smtp_username,
                smtp_password, smtp_from
         FROM instance_settings WHERE id = 1",
    )
    .fetch_one(conn)
    .await
}

pub async fn effective(
    conn: &mut SqliteConnection,
    config: &Config,
) -> Result<Effective, sqlx::Error> {
    let row = row(conn).await?;
    Ok(Effective {
        name: config
            .instance_name
            .clone()
            .or(row.name)
            .unwrap_or_else(|| DEFAULT_NAME.to_owned()),
        public_url: config.public_origin.clone().or(row.public_url),
        timezone: row.timezone.parse().unwrap_or(Tz::UTC),
        tls_expiry_notify_days: parse_tls_days(&row.tls_expiry_notify_days),
    })
}

/// Kuma's server timezone: times in notifications and maintenance without a zone of its own.
pub async fn timezone(conn: &mut SqliteConnection) -> Result<Tz, sqlx::Error> {
    let zone: String = sqlx::query_scalar("SELECT timezone FROM instance_settings WHERE id = 1")
        .fetch_one(conn)
        .await?;
    Ok(zone.parse().unwrap_or(Tz::UTC))
}

/// Kuma's `tlsExpiryNotifyDays`.
pub async fn tls_expiry_notify_days(conn: &mut SqliteConnection) -> Result<Vec<u32>, sqlx::Error> {
    let days: String =
        sqlx::query_scalar("SELECT tls_expiry_notify_days FROM instance_settings WHERE id = 1")
            .fetch_one(conn)
            .await?;
    Ok(parse_tls_days(&days))
}

fn parse_tls_days(stored: &str) -> Vec<u32> {
    serde_json::from_str(stored).unwrap_or_else(|_| DEFAULT_TLS_DAYS.to_vec())
}

/// The instance SMTP server with its password unsealed, for sending.
pub struct Smtp {
    pub host: String,
    pub port: u16,
    pub secure: bool,
    pub ignore_tls_error: bool,
    pub ignore_starttls: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: String,
}

pub async fn smtp(conn: &mut SqliteConnection, key: &SecretKey) -> anyhow::Result<Option<Smtp>> {
    let row = row(conn).await?;
    let (Some(host), Some(port), Some(from)) = (row.smtp_host, row.smtp_port, row.smtp_from) else {
        return Ok(None);
    };
    let password = row
        .smtp_password
        .map(|sealed| {
            let plain = key
                .open(SMTP_PASSWORD, &sealed)
                .ok_or_else(|| anyhow::anyhow!("the instance SMTP password does not open"))?;
            Ok::<_, anyhow::Error>(String::from_utf8(plain)?)
        })
        .transpose()?;
    Ok(Some(Smtp {
        host,
        port: u16::try_from(port)?,
        secure: row.smtp_secure,
        ignore_tls_error: row.smtp_ignore_tls_error,
        ignore_starttls: row.smtp_ignore_starttls,
        username: row.smtp_username,
        password,
        from,
    }))
}

async fn settings(
    conn: &mut SqliteConnection,
    config: &Config,
) -> Result<InstanceSettings, ApiError> {
    let effective = effective(conn, config).await?;
    let row = row(conn).await?;
    let smtp = match (row.smtp_host, row.smtp_port, row.smtp_from) {
        (Some(host), Some(port), Some(from)) => Some(SmtpSettings {
            host,
            port: u16::try_from(port).map_err(ApiError::internal)?,
            secure: row.smtp_secure,
            ignore_tls_error: row.smtp_ignore_tls_error,
            ignore_starttls: row.smtp_ignore_starttls,
            username: row.smtp_username,
            password: SecretField {
                set: row.smtp_password.is_some(),
                ..SecretField::default()
            },
            from,
        }),
        _ => None,
    };
    Ok(InstanceSettings {
        name: effective.name,
        name_fixed: config.instance_name.is_some(),
        public_url: effective.public_url,
        public_url_fixed: config.public_origin.is_some(),
        timezone: effective.timezone.name().to_owned(),
        tls_expiry_notify_days: effective.tls_expiry_notify_days,
        smtp,
    })
}

/// 1 to [`TLS_DAYS_MAX`] values from 1 to [`TLS_DAY_MAX`], kept in the order given as Kuma
/// does, without repeats.
fn tls_days(days: &[u32]) -> Result<Vec<u32>, ApiError> {
    let field = "tls_expiry_notify_days";
    if days.is_empty() {
        return Err(FieldErrors::one(field, FieldErrorCode::Required, None));
    }
    if days.len() > TLS_DAYS_MAX {
        return Err(FieldErrors::one(
            field,
            FieldErrorCode::TooMany,
            Some(TLS_DAYS_MAX as u32),
        ));
    }
    if days.iter().any(|day| !(1..=TLS_DAY_MAX).contains(day)) {
        return Err(FieldErrors::one(field, FieldErrorCode::InvalidValue, None));
    }
    let mut unique = Vec::new();
    for day in days {
        if !unique.contains(day) {
            unique.push(*day);
        }
    }
    Ok(unique)
}

/// The trimmed host and sender of `smtp`, with field errors on `smtp.*` added to `errors`.
fn checked(smtp: &SmtpSettings, mut errors: FieldErrors) -> Result<(&str, &str), ApiError> {
    let host = smtp.host.trim();
    if host.is_empty() {
        errors.add("smtp.host", FieldErrorCode::Required, None);
    }
    if smtp.port == 0 {
        errors.add("smtp.port", FieldErrorCode::InvalidValue, None);
    }
    let from = smtp.from.trim();
    if !is_mailbox(from) {
        errors.add("smtp.from", FieldErrorCode::InvalidFormat, None);
    }
    errors.finish()?;
    Ok((host, from))
}

/// `alerts@example.com` or `Name <alerts@example.com>`. The mail server judges the rest.
fn is_mailbox(text: &str) -> bool {
    let address = match text.rsplit_once('<') {
        Some((_, rest)) => match rest.strip_suffix('>') {
            Some(address) => address,
            None => return false,
        },
        None => text,
    };
    address
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
        && !address.contains(char::is_whitespace)
}

impl InstanceHandlers for AppState {
    async fn instance_update_settings(
        &self,
        ctx: &mut CommandCtx,
        args: InstanceUpdateSettingsArgs,
    ) -> Result<InstanceSettings, ApiError> {
        ctx.require_instance_admin().await?;
        let stored = row(ctx.conn()).await?;
        // A field the configuration fixes keeps its stored value, whatever the form sent.
        let name = match &self.config.instance_name {
            Some(_) => stored.name,
            None => Some(validate_name(
                "name",
                args.name.as_deref().unwrap_or_default(),
            )?),
        };
        let public_url = match &self.config.public_origin {
            Some(_) => stored.public_url,
            None => args
                .public_url
                .as_deref()
                .map(str::trim)
                .filter(|url| !url.is_empty())
                .map(|url| {
                    origin_of(url).map_err(|_| {
                        FieldErrors::one("public_url", FieldErrorCode::InvalidFormat, None)
                    })
                })
                .transpose()?,
        };
        let timezone = match args.timezone.as_deref().map(str::trim) {
            None => stored.timezone,
            Some(zone) => zone
                .parse::<Tz>()
                .map_err(|_| FieldErrors::one("timezone", FieldErrorCode::InvalidValue, None))?
                .name()
                .to_owned(),
        };
        let tls_days = match &args.tls_expiry_notify_days {
            None => stored.tls_expiry_notify_days,
            Some(days) => serde_json::to_string(&tls_days(days)?).map_err(ApiError::internal)?,
        };
        let changed = sqlx::query(
            "UPDATE instance_settings
             SET name = ?1, public_url = ?2, timezone = ?3, tls_expiry_notify_days = ?4,
                 updated_at = ?5
             WHERE id = 1 AND (name IS NOT ?1 OR public_url IS NOT ?2 OR timezone IS NOT ?3
                               OR tls_expiry_notify_days IS NOT ?4)",
        )
        .bind(&name)
        .bind(&public_url)
        .bind(&timezone)
        .bind(&tls_days)
        .bind(ctx.now())
        .execute(ctx.conn())
        .await?
        .rows_affected();
        if changed > 0 {
            ctx.record(&Topic::instance(), "instance.settings_changed", json!({}))
                .await?;
        }
        settings(ctx.conn(), &self.config).await
    }

    async fn instance_update_smtp(
        &self,
        ctx: &mut CommandCtx,
        args: InstanceUpdateSmtpArgs,
    ) -> Result<InstanceSettings, ApiError> {
        ctx.require_instance_admin().await?;
        match args.smtp {
            None => {
                sqlx::query(
                    "UPDATE instance_settings
                     SET smtp_host = NULL, smtp_port = NULL, smtp_secure = 0,
                         smtp_ignore_tls_error = 0, smtp_ignore_starttls = 0,
                         smtp_username = NULL, smtp_password = NULL, smtp_from = NULL,
                         updated_at = ?
                     WHERE id = 1",
                )
                .bind(ctx.now())
                .execute(ctx.conn())
                .await?;
            }
            Some(smtp) => {
                let (host, from) = checked(&smtp, FieldErrors::default())?;
                let stored: Option<Vec<u8>> =
                    sqlx::query_scalar("SELECT smtp_password FROM instance_settings WHERE id = 1")
                        .fetch_one(ctx.conn())
                        .await?;
                let password = match (&smtp.password.value, smtp.password.clear) {
                    (Some(value), _) => Some(self.secrets.seal(SMTP_PASSWORD, value.as_bytes())),
                    (None, true) => None,
                    (None, false) => stored,
                };
                sqlx::query(
                    "UPDATE instance_settings
                     SET smtp_host = ?, smtp_port = ?, smtp_secure = ?, smtp_ignore_tls_error = ?,
                         smtp_ignore_starttls = ?, smtp_username = ?, smtp_password = ?,
                         smtp_from = ?, updated_at = ?
                     WHERE id = 1",
                )
                .bind(host)
                .bind(i64::from(smtp.port))
                .bind(smtp.secure)
                .bind(smtp.ignore_tls_error)
                .bind(smtp.ignore_starttls)
                .bind(
                    smtp.username
                        .as_deref()
                        .map(str::trim)
                        .filter(|name| !name.is_empty()),
                )
                .bind(password)
                .bind(from)
                .bind(ctx.now())
                .execute(ctx.conn())
                .await?;
            }
        }
        ctx.record(&Topic::instance(), "instance.settings_changed", json!({}))
            .await?;
        settings(ctx.conn(), &self.config).await
    }

    async fn instance_test_smtp(
        &self,
        ctx: &mut CommandCtx,
        args: InstanceTestSmtpArgs,
    ) -> Result<ChannelTestQueued, ApiError> {
        let admin = ctx.require_instance_admin().await?;
        self.smtp_tests.check(admin.as_str())?;
        let mut errors = FieldErrors::default();
        let to = args.to.trim();
        if !is_mailbox(to) || to.contains(['<', ',']) {
            errors.add("to", FieldErrorCode::InvalidFormat, None);
        }
        let saved = smtp(ctx.conn(), &self.secrets)
            .await
            .map_err(ApiError::internal)?;
        let channel = match &args.smtp {
            Some(smtp) => {
                let (host, from) = checked(smtp, errors)?;
                SmtpChannel {
                    host: Some(host.to_owned()),
                    port: Some(smtp.port),
                    secure: smtp.secure,
                    ignore_tls_error: smtp.ignore_tls_error,
                    ignore_starttls: smtp.ignore_starttls,
                    username: smtp.username.clone(),
                    password: smtp.password.clone(),
                    from: Some(from.to_owned()),
                    to: Some(to.to_owned()),
                    ..SmtpChannel::default()
                }
            }
            // Without a host the outbox sends through the saved server.
            None if saved.is_some() => {
                errors.finish()?;
                SmtpChannel {
                    to: Some(to.to_owned()),
                    ..SmtpChannel::default()
                }
            }
            None => {
                errors.add("smtp", FieldErrorCode::Required, None);
                return Err(errors.finish().expect_err("an error was added"));
            }
        };
        let stored: Kuma = saved
            .and_then(|saved| saved.password)
            .map(|password| Kuma::from_iter([("smtpPassword".to_owned(), password.into())]))
            .unwrap_or_default();
        let name = effective(ctx.conn(), &self.config).await?.name;
        let kuma = to_kuma(&name, &ChannelSettings::Smtp(channel), Some(&stored))?;
        let root: i64 = sqlx::query_scalar("SELECT id FROM nodes WHERE parent_id IS NULL")
            .fetch_one(ctx.conn())
            .await?;
        let root = NodeId::new(root);
        let now = ctx.now();
        let delivery = queue_test(ctx.conn(), &self.secrets, root, None, &kuma, name, now).await?;
        ctx.record(
            &Topic::channels(root),
            "channel.delivery",
            json!({ "delivery": delivery }),
        )
        .await?;
        Ok(ChannelTestQueued { delivery })
    }

    async fn instance_info(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<InstanceInfo>, ApiError> {
        let effective = effective(ctx.conn(), &self.config).await?;
        Ok(Snapshot {
            topic: Topic::instance(),
            data: InstanceInfo {
                name: effective.name,
                version: env!("CARGO_PKG_VERSION").into(),
                public_url: effective.public_url,
            },
        })
    }

    async fn instance_operations(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<InstanceOperations>, ApiError> {
        ctx.require_instance_admin().await?;
        Ok(Snapshot {
            topic: Topic::instance(),
            data: crate::maintenance::diagnostics::collect(self, ctx.conn())
                .await
                .map_err(ApiError::internal)?,
        })
    }

    async fn instance_settings(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<InstanceSettings>, ApiError> {
        ctx.require_instance_admin().await?;
        Ok(Snapshot {
            topic: Topic::instance(),
            data: settings(ctx.conn(), &self.config).await?,
        })
    }
}
