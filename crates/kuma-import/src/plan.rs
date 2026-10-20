//! The import plan: every Kuma configuration row, mapped to what V1Per creates on the target
//! node or listed in the inventory with a reason. The server translates these structs into
//! its own records (decision 0007); IDs here are Kuma's.

use std::collections::BTreeMap;

use chrono_tz::Tz;
use v1per_checks::{CheckDefinition, Secret};
use v1per_maintenance_schedule::Definition;
use v1per_notify::ChannelConfig;

use crate::record::SourceRecord;

macro_rules! kuma_ids {
    ($($(#[$meta:meta])* $name:ident),* $(,)?) => {$(
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub i64);
    )*};
}

kuma_ids! {
    /// `monitor.id`. Group monitors also name the child node made from them.
    MonitorId,
    /// `notification.id`.
    NotificationId,
    /// `tag.id`.
    TagId,
    /// `maintenance.id`.
    MaintenanceId,
    /// `status_page.id`.
    StatusPageId,
    /// `group.id`: a section of a status page.
    PageGroupId,
    /// `incident.id`.
    IncidentId,
    /// `user.id`.
    UserId,
}

#[derive(Debug)]
pub struct ImportPlan {
    /// Child nodes made from group monitors, parents before children.
    pub nodes: Vec<NodePlan>,
    pub monitors: Vec<MonitorPlan>,
    pub tags: Vec<TagPlan>,
    pub channels: Vec<ChannelPlan>,
    pub maintenance: Vec<MaintenancePlan>,
    pub status_pages: Vec<StatusPagePlan>,
    pub incidents: Vec<IncidentPlan>,
    pub users: Vec<UserPlan>,
    pub settings: SettingsPlan,
    /// Certificate expiry reminders Kuma already sent, so V1Per does not repeat them.
    pub certificate_reminders: Vec<CertificateReminder>,
    pub inventory: Vec<InventoryItem>,
    pub secrets: SecretCounts,
}

/// A node below the target node, made from a Kuma group monitor.
#[derive(Debug, Clone, PartialEq)]
pub struct NodePlan {
    pub group: MonitorId,
    pub name: String,
    /// The parent group's node; `None` is the target node.
    pub parent: Option<MonitorId>,
}

#[derive(Debug)]
pub struct MonitorPlan {
    pub id: MonitorId,
    pub name: String,
    pub description: String,
    /// Kuma's `type`, kept for the report and for unsupported monitors.
    pub kuma_type: String,
    /// The node it lives on, named by its group monitor: its own for a group, its parent's
    /// for anything else. `None` is the target node.
    pub node: Option<MonitorId>,
    /// The group monitor whose status it feeds (Kuma's `parent`).
    pub group: Option<MonitorId>,
    pub kind: MonitorKind,
    pub run: RunState,
    /// Seconds.
    pub interval: u32,
    /// Seconds between checks while pending; Kuma's 0 is mapped to `interval`.
    pub retry_interval: u32,
    pub max_retries: u32,
    /// Down beats between repeated alerts; 0 sends one.
    pub resend_interval: u32,
    /// Seconds, positive and at most `interval`. Equal to a check's own timeout.
    pub timeout: f64,
    /// Equal to a check's own flag.
    pub upside_down: bool,
    /// json-query only: a failed query is down at once.
    pub retry_only_on_status_code_failure: bool,
    /// Certificate expiry alerts are on.
    pub expiry_notification: bool,
    pub tags: Vec<MonitorTag>,
    pub channels: Vec<NotificationId>,
}

#[derive(Debug)]
pub enum MonitorKind {
    Check(Box<CheckDefinition>),
    /// The token of `/api/push/<token>`, kept so push clients keep working.
    Push {
        token: Option<Secret>,
    },
    Group,
    Manual {
        status: Option<ManualStatus>,
    },
    /// A Kuma type V1Per does not run, or a definition that does not validate. The row is
    /// in the inventory.
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualStatus {
    Down,
    Up,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    Active,
    /// Paused in Kuma.
    Paused,
    /// Imported paused because V1Per cannot run it as configured. Its row is in the
    /// inventory, so nothing of the definition is lost.
    PausedUnsupported {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorTag {
    pub tag: TagId,
    /// Empty when the tag has no value.
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagPlan {
    pub id: TagId,
    pub name: String,
    pub color: String,
}

#[derive(Debug)]
pub struct ChannelPlan {
    pub id: NotificationId,
    pub name: String,
    /// Kuma's `config.type`; empty when the config has none.
    pub provider: String,
    /// `None` when the config does not parse; the channel is then inactive.
    pub config: Option<ChannelConfig>,
    /// The JSON to seal: Kuma's config without the form-only `isDefault` and
    /// `applyExisting` keys.
    pub sealed_config: Secret,
    /// Preselected for new monitors in the subtree.
    pub is_default: bool,
    pub state: ChannelState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelState {
    Active,
    Inactive { reason: String },
}

#[derive(Debug)]
pub struct MaintenancePlan {
    pub id: MaintenanceId,
    pub title: String,
    pub description: String,
    /// Validated. Its `server_timezone` is Kuma's `serverTimezone` setting, UTC without one.
    pub definition: Definition,
    pub monitors: Vec<MonitorId>,
    /// Pages that announce it.
    pub status_pages: Vec<StatusPageId>,
}

#[derive(Debug)]
pub struct StatusPagePlan {
    pub id: StatusPageId,
    pub slug: String,
    pub title: String,
    pub description: Option<String>,
    pub icon: String,
    /// Kuma's `theme`: `auto`, `light` or `dark`.
    pub theme: String,
    pub published: bool,
    pub search_engine_index: bool,
    pub show_tags: bool,
    pub show_powered_by: bool,
    pub show_certificate_expiry: bool,
    pub show_only_last_heartbeat: bool,
    pub footer_text: Option<String>,
    /// Seconds.
    pub auto_refresh_interval: Option<u32>,
    pub rss_title: Option<String>,
    /// Plain text in Kuma; hash it with argon2, never store it.
    pub password: Option<Secret>,
    /// `status_page_cname` hostnames.
    pub domains: Vec<String>,
    /// In Kuma's display order.
    pub groups: Vec<PageGroupPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageGroupPlan {
    pub id: PageGroupId,
    pub name: String,
    /// In Kuma's display order.
    pub components: Vec<ComponentPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentPlan {
    pub monitor: MonitorId,
    /// The page links the monitor's URL, or `custom_url` when set.
    pub send_url: bool,
    pub custom_url: Option<String>,
}

/// A Kuma page banner, imported as an incident with one update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentPlan {
    pub id: IncidentId,
    pub page: Option<StatusPageId>,
    pub title: String,
    /// Markdown.
    pub content: String,
    /// Kuma's banner style: `info`, `warning`, `danger`, `primary`, `light` or `dark`.
    pub style: String,
    /// UTC, Unix milliseconds.
    pub created_ms: i64,
    pub updated_ms: Option<i64>,
    /// Resolved unless the banner is still shown.
    pub resolved: bool,
}

#[derive(Debug)]
pub struct UserPlan {
    pub id: UserId,
    pub username: String,
    /// bcrypt hash, verified at the first login and then replaced with argon2. `None` for a
    /// legacy sha1 hash (inventoried) or no password.
    pub password_bcrypt: Option<Secret>,
    pub active: bool,
    /// Kuma's ASCII secret, used as the HMAC key as is. Seal it.
    pub totp_secret: Option<Secret>,
    /// The first Kuma user becomes instance admin.
    pub instance_admin: bool,
    pub timezone: Option<String>,
}

/// Kuma settings with a V1Per equivalent. Every other setting is inventoried.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsPlan {
    pub primary_base_url: Option<String>,
    pub tls_expiry_notify_days: Option<Vec<u32>>,
    /// Kuma's raw history retention; below 1 means "keep forever".
    pub keep_data_period_days: Option<i64>,
    pub timezone: Option<Tz>,
}

impl SettingsPlan {
    pub(crate) fn mapped(&self) -> u64 {
        u64::from(self.primary_base_url.is_some())
            + u64::from(self.tls_expiry_notify_days.is_some())
            + u64::from(self.keep_data_period_days.is_some())
            + u64::from(self.timezone.is_some())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CertificateReminder {
    pub monitor: MonitorId,
    pub days: u32,
}

/// Something not imported, or a monitor imported paused, with its source row.
#[derive(Debug, Clone, PartialEq)]
pub struct InventoryItem {
    pub kind: InventoryKind,
    /// A name to recognise it by, never a secret.
    pub label: String,
    pub reason: String,
    pub source: SourceRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InventoryKind {
    /// A monitor imported paused; the row keeps what V1Per could not map.
    Monitor,
    Channel,
    Maintenance,
    PageGroup,
    PageComponent,
    CustomCss,
    Analytics,
    Setting,
    ApiKey,
    Password,
    TwoFactor,
    Proxy,
    DockerHost,
    RemoteBrowser,
    DomainExpiry,
    TlsInfo,
    SentHistory,
    /// A link row whose other end no longer exists.
    Orphan,
}

impl InventoryKind {
    /// A stable name for APIs and storage.
    pub fn code(self) -> &'static str {
        match self {
            Self::Monitor => "monitor",
            Self::Channel => "channel",
            Self::Maintenance => "maintenance",
            Self::PageGroup => "page_group",
            Self::PageComponent => "page_component",
            Self::CustomCss => "custom_css",
            Self::Analytics => "analytics",
            Self::Setting => "setting",
            Self::ApiKey => "api_key",
            Self::Password => "password",
            Self::TwoFactor => "two_factor",
            Self::Proxy => "proxy",
            Self::DockerHost => "docker_host",
            Self::RemoteBrowser => "remote_browser",
            Self::DomainExpiry => "domain_expiry",
            Self::TlsInfo => "tls_info",
            Self::SentHistory => "sent_history",
            Self::Orphan => "orphan",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Monitor => "paused monitor",
            Self::Channel => "notification",
            Self::Maintenance => "maintenance",
            Self::PageGroup => "status page group",
            Self::PageComponent => "status page component",
            Self::CustomCss => "custom CSS",
            Self::Analytics => "analytics",
            Self::Setting => "setting",
            Self::ApiKey => "API key",
            Self::Password => "password",
            Self::TwoFactor => "2FA secret",
            Self::Proxy => "proxy",
            Self::DockerHost => "docker host",
            Self::RemoteBrowser => "remote browser",
            Self::DomainExpiry => "domain expiry",
            Self::TlsInfo => "TLS info",
            Self::SentHistory => "notification history",
            Self::Orphan => "orphaned link",
        }
    }
}

/// Values that are sealed (or hashed) on import, counted, never shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SecretCounts {
    /// Credential columns of monitors: passwords, tokens, keys, headers, connection strings.
    pub monitor_credentials: u64,
    /// Whole channel configs.
    pub channel_configs: u64,
    pub totp_secrets: u64,
    /// Proxy passwords and remote browser URLs, kept sealed in the inventory.
    pub inventory_credentials: u64,
    /// Status page passwords, hashed with argon2 rather than sealed.
    pub page_passwords: u64,
}

/// Tables whose rows are always mapped; an inventory entry for one of their rows annotates
/// it (a paused monitor, a page's custom CSS) instead of standing in for it.
const ANNOTATED: [&str; 4] = ["monitor", "notification", "status_page", "user"];

impl InventoryItem {
    /// Its source row is mapped too, and the entry records what was lost, like the custom CSS
    /// of a page; otherwise the entry stands in for the row.
    pub fn annotates(&self) -> bool {
        ANNOTATED.contains(&self.source.table())
    }
}

impl ImportPlan {
    /// How many source rows of each table the plan accounts for, mapped or inventoried. A
    /// dry run compares these with the table counts, so a dropped row shows up.
    pub fn rows_by_table(&self) -> BTreeMap<&'static str, u64> {
        let sum = |counts: &mut dyn Iterator<Item = usize>| counts.sum::<usize>() as u64;
        let groups = || self.status_pages.iter().flat_map(|page| &page.groups);
        let mut rows = BTreeMap::from([
            ("monitor", self.monitors.len() as u64),
            (
                "monitor_tag",
                sum(&mut self.monitors.iter().map(|m| m.tags.len())),
            ),
            (
                "monitor_notification",
                sum(&mut self.monitors.iter().map(|m| m.channels.len())),
            ),
            ("tag", self.tags.len() as u64),
            ("notification", self.channels.len() as u64),
            ("maintenance", self.maintenance.len() as u64),
            (
                "monitor_maintenance",
                sum(&mut self.maintenance.iter().map(|m| m.monitors.len())),
            ),
            (
                "maintenance_status_page",
                sum(&mut self.maintenance.iter().map(|m| m.status_pages.len())),
            ),
            ("status_page", self.status_pages.len() as u64),
            (
                "status_page_cname",
                sum(&mut self.status_pages.iter().map(|p| p.domains.len())),
            ),
            ("group", groups().count() as u64),
            (
                "monitor_group",
                sum(&mut groups().map(|g| g.components.len())),
            ),
            ("incident", self.incidents.len() as u64),
            ("user", self.users.len() as u64),
            ("setting", self.settings.mapped()),
            (
                "notification_sent_history",
                self.certificate_reminders.len() as u64,
            ),
        ]);
        for item in self.inventory.iter().filter(|item| !item.annotates()) {
            *rows.entry(item.source.table()).or_default() += 1;
        }
        rows
    }
}
