//! One monitor and its events. On 2026-10-18 Europe/Vienna is UTC+2.

use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use v1per_notify::{
    Beat, CertType, CertificateExpiry, DomainExpiry, Event, Heartbeat, Monitor, MonitorTag,
    ProbeOffline,
};

pub const TZ: Tz = chrono_tz::Europe::Vienna;

pub fn at(hour: u32, min: u32, sec: u32, millis: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 18, hour, min, sec).unwrap()
        + chrono::Duration::milliseconds(millis)
}

pub fn monitor() -> Monitor {
    Monitor {
        id: 7,
        name: "API".into(),
        kind: "http".into(),
        subtype: None,
        url: Some("https://api.example.com/health".into()),
        hostname: None,
        port: None,
        groups: vec!["Production".into()],
        tags: vec![
            MonitorTag {
                tag_id: 1,
                name: "prod".into(),
                value: String::new(),
                color: "#ff0000".into(),
            },
            MonitorTag {
                tag_id: 2,
                name: "region".into(),
                value: "frankfurt".into(),
                color: "#00ff00".into(),
            },
        ],
        link: Some("https://v1per.example.com/monitors/7".into()),
    }
}

/// Down at 12:03:11.123 UTC.
pub fn down() -> Event {
    Event::MonitorDown(Beat {
        monitor: monitor(),
        heartbeat: Heartbeat {
            time: at(12, 3, 11, 123),
            msg: "timeout of 48000ms exceeded".into(),
            ping: None,
            important: true,
            duration: 60,
            retries: 0,
            response: None,
            last_down_time: None,
        },
        timezone: TZ,
    })
}

/// Up at 13:05:16.456 UTC after the down above: 1h 2m 5s of downtime.
pub fn up() -> Event {
    Event::MonitorUp(Beat {
        monitor: monitor(),
        heartbeat: Heartbeat {
            time: at(13, 5, 16, 456),
            msg: "200 - OK".into(),
            ping: Some(123),
            important: true,
            duration: 60,
            retries: 0,
            response: None,
            last_down_time: Some(at(12, 3, 11, 123)),
        },
        timezone: TZ,
    })
}

pub fn certificate() -> Event {
    Event::CertificateExpiry(CertificateExpiry {
        monitor_name: "API".into(),
        monitor_url: "https://api.example.com/health".into(),
        cert_type: CertType::Server,
        subject_cn: "api.example.com".into(),
        days_remaining: 13,
    })
}

pub fn domain() -> Event {
    Event::DomainExpiry(DomainExpiry {
        domain: "example.com".into(),
        days_remaining: 20,
    })
}

pub fn probe_offline() -> Event {
    Event::ProbeOffline(ProbeOffline {
        probe_name: "Frankfurt".into(),
        last_seen: at(12, 3, 11, 0),
        timezone: TZ,
    })
}

pub fn test(channel_name: &str) -> Event {
    Event::Test {
        channel_name: channel_name.into(),
    }
}

/// Unix seconds of the down beat and of the up beat.
pub const DOWN_UNIX: i64 = 1_792_324_991;
pub const UP_UNIX: i64 = 1_792_328_716;

pub const CERT_MSG: &str = "[API][https://api.example.com/health] server certificate api.example.com will expire in 13 days";
pub const DOMAIN_MSG: &str = "Domain name example.com will expire in 20 days";
pub const PROBE_MSG: &str = "Probe Frankfurt is offline since 2026-10-18 14:03:11 (Europe/Vienna)";
