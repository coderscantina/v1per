//! Certificate expiry alerts, as Kuma 2.5.0 sends them (`server/util-server.js:940-977`,
//! `server/model/monitor.js:1545-1579`, contract "Certificate expiry"). For each threshold of
//! the instance's `tls_expiry_notify_days`, in stored order, the chain is walked from the
//! server certificate up to a trusted root; a certificate within the threshold alerts unless
//! this monitor already alerted at that threshold or a lower one. The thresholds alerted are
//! forgotten when the server certificate changes.
//!
//! Kuma records a threshold once a channel accepted the message; here it is recorded when the
//! message is queued, since the outbox delivers it.

use sqlx::SqliteConnection;
use v1per_checks::CertType;
use v1per_protocol::envelope::Topic;
use v1per_protocol::ids::MonitorId;

use super::outbox::{self, CertKind, CertificatePayload};
use crate::instance;
use crate::probes::wire::TlsChain;

const DAY_MS: i64 = 86_400_000;

/// Checks the chain a probe saw at `checked_at` for monitor `monitor` at `url`, and queues
/// the alerts due. Returns the topics to wake.
pub(crate) async fn check(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
    url: &str,
    chain: &TlsChain,
    checked_at: i64,
    now: i64,
) -> sqlx::Result<Vec<Topic>> {
    let Some(server) = chain.certificates.first() else {
        return Ok(Vec::new());
    };
    let seen: Option<String> =
        sqlx::query_scalar("SELECT fingerprint FROM monitor_certificates WHERE monitor_id = ?")
            .bind(monitor.get())
            .fetch_optional(&mut *conn)
            .await?;
    if seen.as_deref() != Some(server.fingerprint.as_str()) {
        // An imported history has no fingerprint yet; it stays until the certificate changes.
        if seen.is_some() {
            sqlx::query("DELETE FROM certificate_alerts WHERE monitor_id = ?")
                .bind(monitor.get())
                .execute(&mut *conn)
                .await?;
        }
        sqlx::query(
            "INSERT INTO monitor_certificates (monitor_id, fingerprint) VALUES (?, ?)
             ON CONFLICT (monitor_id) DO UPDATE SET fingerprint = excluded.fingerprint",
        )
        .bind(monitor.get())
        .bind(&server.fingerprint)
        .execute(&mut *conn)
        .await?;
    }

    let thresholds = instance::tls_expiry_notify_days(conn).await?;
    let name: String = sqlx::query_scalar("SELECT name FROM monitors WHERE id = ?")
        .bind(monitor.get())
        .fetch_one(&mut *conn)
        .await?;
    let mut topics = Vec::new();
    for threshold in thresholds {
        let threshold = i64::from(threshold);
        for certificate in &chain.certificates {
            // A root the probe trusts is the operating system's to renew.
            if certificate.cert_type == CertType::RootCa && chain.valid {
                break;
            }
            let days_remaining = (certificate.valid_to - checked_at) / DAY_MS;
            if days_remaining > threshold {
                continue;
            }
            let alerted: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM certificate_alerts WHERE monitor_id = ? AND days <= ? LIMIT 1",
            )
            .bind(monitor.get())
            .bind(threshold)
            .fetch_optional(&mut *conn)
            .await?;
            if alerted.is_some() {
                continue;
            }
            let payload = CertificatePayload {
                monitor_name: name.clone(),
                monitor_url: url.to_owned(),
                cert_type: match certificate.cert_type {
                    CertType::Server => CertKind::Server,
                    CertType::SelfSigned => CertKind::SelfSigned,
                    CertType::IntermediateCa => CertKind::IntermediateCa,
                    CertType::RootCa => CertKind::RootCa,
                },
                subject_cn: certificate.subject.clone(),
                days_remaining,
            };
            let (queued, woken) = outbox::queue_certificate(conn, monitor, payload, now).await?;
            for topic in woken {
                if !topics.contains(&topic) {
                    topics.push(topic);
                }
            }
            if queued {
                sqlx::query(
                    "INSERT OR IGNORE INTO certificate_alerts (monitor_id, days) VALUES (?, ?)",
                )
                .bind(monitor.get())
                .bind(threshold)
                .execute(&mut *conn)
                .await?;
            }
        }
    }
    Ok(topics)
}
