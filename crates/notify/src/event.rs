//! Events and the `msg`, `monitorJSON` and `heartbeatJSON` Kuma derives from them.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use crate::js::Js;

/// Something to tell a channel about.
#[derive(Clone, Debug)]
pub enum Event {
    MonitorDown(Beat),
    MonitorUp(Beat),
    CertificateExpiry(CertificateExpiry),
    DomainExpiry(DomainExpiry),
    /// V1Per only: Kuma has a single check location.
    ProbeOffline(ProbeOffline),
    /// The channel dialog's test button. Kuma applies email templates to it.
    Test {
        channel_name: String,
    },
}

/// A monitor's state change, with the beat that caused it.
#[derive(Clone, Debug)]
pub struct Beat {
    pub monitor: Monitor,
    pub heartbeat: Heartbeat,
    /// Zone for `localDateTime`, Kuma's server timezone.
    pub timezone: Tz,
}

/// The monitor fields Kuma's providers and templates read (`monitor.toJSON`).
#[derive(Clone, Debug)]
pub struct Monitor {
    pub id: i64,
    pub name: String,
    /// Kuma's monitor type, for example `http`, `keyword`, `port`, `ping`, `push`, `group`.
    pub kind: String,
    pub subtype: Option<String>,
    pub url: Option<String>,
    pub hostname: Option<String>,
    pub port: Option<u16>,
    /// Ancestor group names, outermost first.
    pub groups: Vec<String>,
    pub tags: Vec<MonitorTag>,
    /// Absolute link to this monitor in V1Per, where Kuma links `/dashboard/<id>`.
    pub link: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MonitorTag {
    pub tag_id: i64,
    pub name: String,
    /// Empty when the tag has no value.
    pub value: String,
    pub color: String,
}

/// The beat fields of Kuma's `heartbeatJSON`.
#[derive(Clone, Debug)]
pub struct Heartbeat {
    pub time: DateTime<Utc>,
    /// The check message; may be empty.
    pub msg: String,
    pub ping: Option<i64>,
    pub important: bool,
    /// Seconds since the previous beat.
    pub duration: i64,
    pub retries: i64,
    pub response: Option<String>,
    /// Up events only: when the latest important down beat happened.
    pub last_down_time: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct CertificateExpiry {
    pub monitor_name: String,
    pub monitor_url: String,
    pub cert_type: CertType,
    /// The certificate subject's common name.
    pub subject_cn: String,
    /// Whole days left, truncated toward zero.
    pub days_remaining: i64,
}

/// Position of the expiring certificate in the chain (`server/util-server.js:477-486`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertType {
    Server,
    IntermediateCa,
    RootCa,
    SelfSigned,
}

impl CertType {
    fn label(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::IntermediateCa => "intermediate CA",
            Self::RootCa => "root CA",
            Self::SelfSigned => "self-signed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DomainExpiry {
    pub domain: String,
    /// Whole days left, truncated toward zero.
    pub days_remaining: i64,
}

#[derive(Clone, Debug)]
pub struct ProbeOffline {
    pub probe_name: String,
    pub last_seen: DateTime<Utc>,
    pub timezone: Tz,
}

/// Kuma's `(msg, monitorJSON, heartbeatJSON)` triple plus the typed beat behind it.
pub(crate) struct Notice<'a> {
    pub msg: String,
    pub beat: Option<BeatNotice<'a>>,
    pub monitor_json: Js,
    pub heartbeat_json: Js,
}

pub(crate) struct BeatNotice<'a> {
    pub up: bool,
    pub monitor: &'a Monitor,
    pub heartbeat: &'a Heartbeat,
    /// `heartbeatJSON.msg`: the check message, `N/A` when empty.
    pub msg: &'a str,
    pub timezone: &'static str,
    pub local_date_time: String,
}

impl BeatNotice<'_> {
    /// `<name> / <name>` of the enclosing groups, empty for top-level monitors.
    pub fn group_path(&self) -> String {
        self.monitor.groups.join(" / ")
    }
}

/// Kuma's `R.isoDateTimeMillis` format, used for `time` and `lastDownTime`.
fn db_time(time: &DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// Kuma's `SQL_DATETIME_FORMAT` in the given zone.
fn local_time(time: &DateTime<Utc>, tz: Tz) -> String {
    time.with_timezone(&tz)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

impl Event {
    /// Kuma's `msg` for this event, such as `[API] [🔴 Down] timeout`, for logs and lists.
    pub fn message(&self) -> String {
        self.notice().msg
    }

    pub(crate) fn notice(&self) -> Notice<'_> {
        match self {
            Self::MonitorDown(beat) => beat.notice(false),
            Self::MonitorUp(beat) => beat.notice(true),
            Self::CertificateExpiry(cert) => Notice::general(format!(
                "[{}][{}] {} certificate {} will expire in {} days",
                cert.monitor_name,
                cert.monitor_url,
                cert.cert_type.label(),
                cert.subject_cn,
                cert.days_remaining
            )),
            Self::DomainExpiry(domain) => Notice::general(format!(
                "Domain name {} will expire in {} days",
                domain.domain, domain.days_remaining
            )),
            Self::ProbeOffline(probe) => Notice::general(format!(
                "Probe {} is offline since {} ({})",
                probe.probe_name,
                local_time(&probe.last_seen, probe.timezone),
                probe.timezone.name()
            )),
            Self::Test { channel_name } => Notice::general(format!("{channel_name} Testing")),
        }
    }
}

impl Notice<'_> {
    /// Events without a monitor or beat: Kuma passes `null` for both objects.
    fn general(msg: String) -> Self {
        Self {
            msg,
            beat: None,
            monitor_json: Js::Null,
            heartbeat_json: Js::Null,
        }
    }
}

impl Beat {
    /// `server/model/monitor.js:1456-1503`.
    fn notice(&self, up: bool) -> Notice<'_> {
        let Self {
            monitor,
            heartbeat,
            timezone,
        } = self;
        let label = if up { "✅ Up" } else { "🔴 Down" };
        let msg = format!("[{}] [{label}] {}", monitor.name, heartbeat.msg);
        let beat_msg = if heartbeat.msg.is_empty() {
            "N/A"
        } else {
            &heartbeat.msg
        };
        let local_date_time = local_time(&heartbeat.time, *timezone);

        let mut heartbeat_json = vec![
            ("monitorID", Js::Int(monitor.id)),
            ("status", Js::Int(if up { 1 } else { 0 })),
            ("time", db_time(&heartbeat.time).into()),
            ("msg", beat_msg.into()),
            ("ping", heartbeat.ping.into()),
            ("important", heartbeat.important.into()),
            ("duration", heartbeat.duration.into()),
            ("retries", heartbeat.retries.into()),
            ("response", heartbeat.response.clone().into()),
            ("timezone", timezone.name().into()),
            (
                "timezoneOffset",
                heartbeat
                    .time
                    .with_timezone(timezone)
                    .format("%:z")
                    .to_string()
                    .into(),
            ),
            ("localDateTime", local_date_time.clone().into()),
        ];
        if up && let Some(last_down) = &heartbeat.last_down_time {
            heartbeat_json.push(("lastDownTime", db_time(last_down).into()));
        }

        Notice {
            msg,
            monitor_json: monitor.to_js(),
            heartbeat_json: Js::object(heartbeat_json),
            beat: Some(BeatNotice {
                up,
                monitor,
                heartbeat,
                msg: beat_msg,
                timezone: timezone.name(),
                local_date_time,
            }),
        }
    }
}

impl Monitor {
    /// The documented subset of `monitor.toJSON(preloadData, false)`, in Kuma's key order.
    fn to_js(&self) -> Js {
        let mut path = self.groups.clone();
        path.push(self.name.clone());
        let tags = self
            .tags
            .iter()
            .map(|tag| {
                Js::object([
                    ("tag_id", Js::Int(tag.tag_id)),
                    ("monitor_id", Js::Int(self.id)),
                    ("value", tag.value.as_str().into()),
                    ("name", tag.name.as_str().into()),
                    ("color", tag.color.as_str().into()),
                ])
            })
            .collect();
        Js::object([
            ("id", Js::Int(self.id)),
            ("name", self.name.as_str().into()),
            ("path", path.clone().into()),
            ("pathName", path.join(" / ").into()),
            ("url", self.url.clone().into()),
            ("hostname", self.hostname.clone().into()),
            ("port", self.port.map(i64::from).into()),
            ("type", self.kind.as_str().into()),
            ("subtype", self.subtype.clone().into()),
            ("tags", Js::Array(tags)),
            ("includeSensitiveData", false.into()),
        ])
    }
}

/// Kuma's `extractAddress` (`notification-provider.js:32-65`), over `monitorJSON`.
pub(crate) fn extract_address(monitor: &Js) -> String {
    if monitor.is_null() {
        return String::new();
    }
    let field = |key| monitor.get(key).to_string();
    match monitor.get("type").as_str().unwrap_or_default() {
        "push" => "Heartbeat".to_owned(),
        "ping" | "dns" => field("hostname"),
        "port" | "gamedig" | "steam" => {
            if monitor.get("port").truthy() {
                format!("{}:{}", field("hostname"), field("port"))
            } else {
                field("hostname")
            }
        }
        "globalping" => match monitor.get("subtype").as_str().unwrap_or_default() {
            "ping" | "dns" => field("hostname"),
            "http" => field("url"),
            _ => String::new(),
        },
        _ => {
            let url = field("url");
            if matches!(url.as_str(), "https://" | "http://") {
                String::new()
            } else {
                url
            }
        }
    }
}
