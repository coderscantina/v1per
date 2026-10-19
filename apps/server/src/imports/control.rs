//! The `control` step: every configuration record of the plan, the inventory and the step's
//! checkpoint in one control-database transaction, so an interrupted run has all of it or
//! none (decision 0007).

use std::collections::{BTreeMap, BTreeSet, HashSet};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqliteConnection;
use v1per_checks as checks;
use v1per_kuma_import::{
    ChannelState, ImportPlan, ManualStatus, MonitorId as KumaMonitor, MonitorKind, MonitorPlan,
    RunState, StatusPagePlan, UserPlan,
};
use v1per_maintenance_schedule::{DayOfMonth, Definition, Strategy, Timezone};
use v1per_protocol::ids::{ChannelId, MonitorId, NodeId, UserId};
use v1per_protocol::limits::{
    MONITOR_DESCRIPTION_MAX_CHARS, MONITOR_TAG_MAX_CHARS, MONITOR_TAGS_MAX, NAME_MAX_CHARS,
    NODE_DEPTH_MAX, PAGE_DESCRIPTION_MAX_CHARS, PAGE_FOOTER_MAX_CHARS, PAGE_HOSTNAMES_MAX,
};
use v1per_protocol::operations::imports::{ImportStep, ImportedLogin, ImportedPassword};
use v1per_protocol::operations::monitors::{
    DnsRecordType, DnsSettings, HttpAuth, HttpBodyEncoding, HttpHeader, HttpSettings, IpFamily,
    JsonPathOperator, JsonQuerySettings, KeywordSettings, MonitorInput, MonitorSettings,
    OAuthClientAuth, PingSettings, PortSettings, PushSettings, Quorum, SecretField,
    UnsupportedSettings,
};

use super::Options;
use super::state::{self, Advance};
use crate::identity::secrets::SecretKey;
use crate::identity::{self, token_digest};
use crate::incidents::maintenance;
use crate::monitors::{self, settings};
use crate::notifications::config::channel_context;
use crate::pages::{normalize_hostname, valid_slug};

/// Longest page title and maintenance or incident title v1per accepts.
const PAGE_TITLE_MAX: usize = 100;
const TITLE_MAX: usize = 200;
/// The role Kuma users get on the target node: Kuma has no roles, everyone manages everything.
const IMPORTED_ROLE: &str = "owner";

/// The node an import writes into.
pub struct Target {
    pub node: NodeId,
    /// Levels below the root.
    pub depth: u32,
}

/// Why the plan cannot go into `target`, one line per problem. Empty when it can.
pub async fn preflight(
    conn: &mut SqliteConnection,
    target: &Target,
    plan: &ImportPlan,
    options: &Options,
) -> sqlx::Result<Vec<String>> {
    let mut problems = Vec::new();
    let occupied: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM nodes WHERE parent_id = ?1)
              + (SELECT count(*) FROM monitors WHERE node_id = ?1)
              + (SELECT count(*) FROM channels WHERE node_id = ?1)
              + (SELECT count(*) FROM maintenances WHERE node_id = ?1)
              + (SELECT count(*) FROM components WHERE node_id = ?1)
              + (SELECT count(*) FROM incidents WHERE node_id = ?1)
              + (SELECT count(*) FROM pages WHERE node_id = ?1)
              + (SELECT count(*) FROM probes WHERE node_id = ?1 AND builtin = 0)",
    )
    .bind(target.node.get())
    .fetch_one(&mut *conn)
    .await?;
    if occupied > 0 {
        problems.push(format!(
            "node {} is not empty: import into a new node without children, monitors, channels, maintenance, components, incidents, pages or probes",
            target.node
        ));
    }

    let depth = group_depths(plan).into_values().max().unwrap_or(0);
    if target.depth + depth > NODE_DEPTH_MAX {
        problems.push(format!(
            "Kuma groups nest {depth} levels deep; below node {} the tree allows {}",
            target.node,
            NODE_DEPTH_MAX.saturating_sub(target.depth)
        ));
    }

    let mut logins = BTreeSet::new();
    for user in &plan.users {
        let email = login(&user.username, &options.login_domain);
        if !logins.insert(email.clone()) {
            problems.push(format!("two Kuma users sign in as `{email}`"));
        } else if exists(conn, "SELECT 1 FROM users WHERE email = ?", &email).await? {
            problems.push(format!(
                "the login `{email}` of Kuma user `{}` exists already",
                user.username
            ));
        }
    }

    let mut slugs = BTreeSet::new();
    for page in &plan.status_pages {
        let slug = page.slug.trim().to_lowercase();
        if !valid_slug(&slug) {
            problems.push(format!(
                "status page slug `{}` is not a valid v1per slug",
                page.slug
            ));
        } else if !slugs.insert(slug.clone())
            || exists(conn, "SELECT 1 FROM pages WHERE slug = ?", &slug).await?
        {
            problems.push(format!("the status page slug `{slug}` is taken"));
        }
        for hostname in page.domains.iter().filter_map(|d| normalize_hostname(d)) {
            if exists(
                conn,
                "SELECT 1 FROM page_hostnames WHERE hostname = ?",
                &hostname,
            )
            .await?
            {
                problems.push(format!(
                    "the hostname `{hostname}` of status page `{slug}` belongs to another page"
                ));
            }
        }
    }

    for monitor in &plan.monitors {
        if let MonitorKind::Push { token: Some(token) } = &monitor.kind
            && exists(
                conn,
                "SELECT 1 FROM monitors WHERE push_token_hash = ?",
                &token_digest(token.expose()),
            )
            .await?
        {
            problems.push(format!(
                "the push token of monitor `{}` is used by another monitor",
                monitor.name
            ));
        }
    }
    Ok(problems)
}

async fn exists(conn: &mut SqliteConnection, sql: &'static str, value: &str) -> sqlx::Result<bool> {
    Ok(sqlx::query_scalar::<_, i64>(sql)
        .bind(value)
        .fetch_optional(conn)
        .await?
        .is_some())
}

/// Levels each group's node sits below the target node.
fn group_depths(plan: &ImportPlan) -> BTreeMap<KumaMonitor, u32> {
    let mut depths = BTreeMap::new();
    // Parents come before children in the plan.
    for node in &plan.nodes {
        let parent = node
            .parent
            .and_then(|p| depths.get(&p).copied())
            .unwrap_or(0);
        depths.insert(node.group, parent + 1);
    }
    depths
}

/// The login of a Kuma username: itself when it is an email address.
pub fn login(username: &str, domain: &str) -> String {
    let username = identity::normalize_email(username);
    if username.contains('@') {
        username
    } else {
        format!("{username}@{}", domain.trim().to_lowercase())
    }
}

/// What the control step created that the manifest reports. Kept as the step's checkpoint,
/// so a resumed run reports it too.
#[derive(Default, Serialize, Deserialize)]
pub struct Written {
    pub logins: Vec<ImportedLogin>,
    pub notes: Vec<String>,
}

/// Writes the plan into `target` and checkpoints the step, in `conn`'s transaction.
pub async fn write(
    conn: &mut SqliteConnection,
    key: &SecretKey,
    run: i64,
    target: &Target,
    plan: &ImportPlan,
    options: &Options,
    now: i64,
) -> anyhow::Result<Written> {
    let mut notes = Notes::default();

    // Nodes from group monitors, parents first.
    let mut nodes: BTreeMap<KumaMonitor, NodeId> = BTreeMap::new();
    for node in &plan.nodes {
        let parent = node
            .parent
            .and_then(|group| nodes.get(&group).copied())
            .unwrap_or(target.node);
        let name = notes.clip("group", &node.name, NAME_MAX_CHARS);
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO nodes (parent_id, name, position, created_at)
             VALUES (?1, ?2, (SELECT count(*) FROM nodes WHERE parent_id = ?1), ?3) RETURNING id",
        )
        .bind(parent.get())
        .bind(name)
        .bind(now)
        .fetch_one(&mut *conn)
        .await?;
        nodes.insert(node.group, NodeId::new(id));
        state::record_object(conn, run, "node", node.group.0, &id.to_string()).await?;
    }
    let node_of = |group: Option<KumaMonitor>| {
        group
            .and_then(|group| nodes.get(&group).copied())
            .unwrap_or(target.node)
    };

    // Channels on the target node, so every imported monitor can use them.
    let mut channels: BTreeMap<i64, ChannelId> = BTreeMap::new();
    for channel in &plan.channels {
        let (active, reason) = match (&channel.state, &channel.config) {
            (ChannelState::Active, Some(_)) => (true, None),
            (ChannelState::Active, None) => (false, Some("the configuration does not parse")),
            (ChannelState::Inactive { reason }, _) => (false, Some(reason.as_str())),
        };
        let name = notes.clip("channel", &channel.name, NAME_MAX_CHARS);
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO channels (node_id, name, provider, config, is_default, active,
                                   inactive_reason, revision, created_at, updated_at)
             VALUES (?, ?, ?, x'', ?, ?, ?, 1, ?, ?) RETURNING id",
        )
        .bind(target.node.get())
        .bind(name)
        .bind(&channel.provider)
        .bind(channel.is_default)
        .bind(active)
        .bind(reason)
        .bind(now)
        .bind(now)
        .fetch_one(&mut *conn)
        .await?;
        let channel_id = ChannelId::new(id);
        let sealed = key.seal(
            &channel_context(channel_id),
            channel.sealed_config.expose().as_bytes(),
        );
        sqlx::query("UPDATE channels SET config = ? WHERE id = ?")
            .bind(sealed)
            .bind(id)
            .execute(&mut *conn)
            .await?;
        channels.insert(channel.id.0, channel_id);
        state::record_object(conn, run, "channel", channel.id.0, &id.to_string()).await?;
    }

    // Monitors, then their groups once every group exists.
    let tags: BTreeMap<i64, &str> = plan
        .tags
        .iter()
        .map(|tag| (tag.id.0, tag.name.as_str()))
        .collect();
    if plan.monitors.iter().any(|m| !m.tags.is_empty()) {
        notes.add("tag colors are not kept; tags become `name` or `name:value` labels");
    }
    let builtin = crate::probes::builtin(conn).await?;
    let mut monitor_ids: BTreeMap<i64, MonitorId> = BTreeMap::new();
    let mut on_probes = BTreeSet::new();
    for monitor in &plan.monitors {
        let settings = settings_of(monitor);
        if monitors::runs_on_probes(&settings) {
            on_probes.insert(monitor.id.0);
        }
        let node = node_of(monitor.node);
        let id = write_monitor(conn, key, monitor, settings, node, &tags, &mut notes, now)
            .await
            .with_context(|| format!("monitor `{}`", monitor.name))?;
        monitor_ids.insert(monitor.id.0, id);
        state::record_object(conn, run, "monitor", monitor.id.0, &id.to_string()).await?;
    }
    for monitor in &plan.monitors {
        let id = monitor_ids[&monitor.id.0];
        if let Some(group) = monitor.group.and_then(|g| monitor_ids.get(&g.0)) {
            sqlx::query("UPDATE monitors SET group_id = ? WHERE id = ?")
                .bind(group.get())
                .bind(id.get())
                .execute(&mut *conn)
                .await?;
        }
        if on_probes.contains(&monitor.id.0) {
            sqlx::query("INSERT INTO monitor_probes (monitor_id, probe_id) VALUES (?, ?)")
                .bind(id.get())
                .bind(builtin.get())
                .execute(&mut *conn)
                .await?;
        }
        for channel in monitor.channels.iter().filter_map(|c| channels.get(&c.0)) {
            sqlx::query("INSERT INTO monitor_channels (monitor_id, channel_id) VALUES (?, ?)")
                .bind(id.get())
                .bind(channel.get())
                .execute(&mut *conn)
                .await?;
        }
    }
    for reminder in &plan.certificate_reminders {
        if let Some(id) = monitor_ids.get(&reminder.monitor.0) {
            sqlx::query(
                "INSERT OR IGNORE INTO certificate_alerts (monitor_id, days) VALUES (?, ?)",
            )
            .bind(id.get())
            .bind(reminder.days)
            .execute(&mut *conn)
            .await?;
        }
    }

    // Maintenance on the target node, covering its monitors.
    for window in &plan.maintenance {
        let stored = StoredSchedule::of(&window.definition);
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO maintenances (node_id, title, description, strategy, active, start_date,
                 end_date, start_time, end_time, weekdays, days_of_month, interval_day, cron,
                 duration, timezone, revision, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?) RETURNING id",
        )
        .bind(target.node.get())
        .bind(notes.clip("maintenance", &window.title, TITLE_MAX))
        .bind(&window.description)
        .bind(stored.strategy)
        .bind(window.definition.active)
        .bind(stored.start_date)
        .bind(stored.end_date)
        .bind(stored.start_time)
        .bind(stored.end_time)
        .bind(stored.weekdays)
        .bind(stored.days_of_month)
        .bind(window.definition.interval_day)
        .bind(&window.definition.cron)
        .bind(window.definition.duration)
        .bind(stored.timezone)
        .bind(now)
        .bind(now)
        .fetch_one(&mut *conn)
        .await?;
        for monitor in window.monitors.iter().filter_map(|m| monitor_ids.get(&m.0)) {
            sqlx::query(
                "INSERT OR IGNORE INTO maintenance_monitors (maintenance_id, monitor_id)
                 VALUES (?, ?)",
            )
            .bind(id)
            .bind(monitor.get())
            .execute(&mut *conn)
            .await?;
        }
        state::record_object(conn, run, "maintenance", window.id.0, &id.to_string()).await?;
    }

    let mut pages: BTreeMap<i64, i64> = BTreeMap::new();
    let page_timezone = match plan.settings.timezone {
        Some(zone) => zone.name().to_owned(),
        None => crate::instance::timezone(conn).await?.name().to_owned(),
    };
    for page in &plan.status_pages {
        let id = write_page(
            conn,
            page,
            target.node,
            &monitor_ids,
            &page_timezone,
            &mut notes,
            now,
        )
        .await
        .with_context(|| format!("status page `{}`", page.slug))?;
        pages.insert(page.id.0, id);
        state::record_object(conn, run, "page", page.id.0, &id.to_string()).await?;
    }

    // A Kuma banner is an incident with one update, published to its page.
    for incident in &plan.incidents {
        let state = if incident.resolved {
            "resolved"
        } else {
            "investigating"
        };
        let resolved_at = incident
            .resolved
            .then(|| incident.updated_ms.unwrap_or(incident.created_ms));
        let updated = incident.updated_ms.unwrap_or(incident.created_ms);
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO incidents (node_id, title, state, auto, started_at, resolved_at,
                                    revision, updated_at)
             VALUES (?, ?, ?, 0, ?, ?, 1, ?) RETURNING id",
        )
        .bind(target.node.get())
        .bind(notes.clip("incident", &incident.title, TITLE_MAX))
        .bind(state)
        .bind(incident.created_ms)
        .bind(resolved_at)
        .bind(updated)
        .fetch_one(&mut *conn)
        .await?;
        sqlx::query(
            "INSERT INTO incident_updates (incident_id, state, message, author_id, created_at)
             VALUES (?, ?, ?, NULL, ?)",
        )
        .bind(id)
        .bind(state)
        .bind(&incident.content)
        .bind(updated)
        .execute(&mut *conn)
        .await?;
        if let Some(page) = incident.page.and_then(|p| pages.get(&p.0)) {
            sqlx::query(
                "INSERT INTO incident_publications (incident_id, page_id, published_at)
                 VALUES (?, ?, ?)",
            )
            .bind(id)
            .bind(page)
            .bind(incident.created_ms)
            .execute(&mut *conn)
            .await?;
        }
        state::record_object(conn, run, "incident", incident.id.0, &id.to_string()).await?;
    }
    if !plan.incidents.is_empty() {
        notes.add("Kuma banner styles are not kept; imported incidents name no affected monitor");
    }

    let mut logins = Vec::new();
    for user in &plan.users {
        let (id, login) = write_user(conn, key, user, target.node, options, now).await?;
        logins.push(login);
        state::record_object(conn, run, "user", user.id.0, id.as_str()).await?;
    }

    write_settings(conn, plan, &mut notes, now).await?;

    for item in &plan.inventory {
        let source = &item.source;
        let columns: Vec<&String> = source.expose().keys().collect();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO import_inventory (run_id, kind, label, reason, source_table, source_id,
                                           columns, sealed)
             VALUES (?, ?, ?, ?, ?, ?, ?, x'') RETURNING id",
        )
        .bind(run)
        .bind(item.kind.code())
        .bind(&item.label)
        .bind(&item.reason)
        .bind(source.table())
        .bind(source.id())
        .bind(serde_json::to_string(&columns)?)
        .fetch_one(&mut *conn)
        .await?;
        let plain = Value::Object(source.expose().clone()).to_string();
        sqlx::query("UPDATE import_inventory SET sealed = ? WHERE id = ?")
            .bind(key.seal(&inventory_context(id), plain.as_bytes()))
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }

    // Maintenance that is on now covers its monitors from the first start.
    maintenance::refresh(conn, now).await?;
    let written = Written {
        logins,
        notes: notes.0.into_iter().collect(),
    };
    state::advance(
        conn,
        run,
        ImportStep::Control,
        Advance {
            cursor: Some(&serde_json::to_string(&written)?),
            chunks: 1,
            rows: plan.monitors.len() as i64,
            skipped: 0,
            done: true,
        },
    )
    .await?;
    Ok(written)
}

/// The sealing context of an inventory entry's source row.
pub fn inventory_context(id: i64) -> String {
    format!("import-inventory:{id}")
}

/// Losses without an inventory row, each said once.
#[derive(Default)]
struct Notes(BTreeSet<String>);

impl Notes {
    fn add(&mut self, note: impl Into<String>) {
        self.0.insert(note.into());
    }

    /// `text` trimmed and cut to `max` characters, noting a cut.
    fn clip(&mut self, what: &str, text: &str, max: usize) -> String {
        let text = text.trim();
        if text.chars().count() <= max {
            return text.to_owned();
        }
        self.add(format!(
            "a {what} name or title longer than {max} characters was shortened"
        ));
        text.chars().take(max).collect()
    }
}

#[allow(clippy::too_many_arguments)]
async fn write_monitor(
    conn: &mut SqliteConnection,
    key: &SecretKey,
    monitor: &MonitorPlan,
    settings: MonitorSettings,
    node: NodeId,
    tags: &BTreeMap<i64, &str>,
    notes: &mut Notes,
    now: i64,
) -> anyhow::Result<MonitorId> {
    let mut labels: Vec<String> = Vec::new();
    for tag in &monitor.tags {
        let name = tags.get(&tag.tag.0).copied().unwrap_or_default();
        let label = match tag.value.trim() {
            "" => name.trim().to_owned(),
            value => format!("{}:{value}", name.trim()),
        };
        if label.is_empty() || labels.contains(&label) {
            continue;
        }
        if labels.len() == MONITOR_TAGS_MAX {
            notes.add(format!(
                "monitors keep at most {MONITOR_TAGS_MAX} tags; the rest were dropped"
            ));
            break;
        }
        labels.push(notes.clip("tag", &label, MONITOR_TAG_MAX_CHARS));
    }
    let manual_status = match &monitor.kind {
        MonitorKind::Manual { status } => status.map(|status| match status {
            ManualStatus::Down => 0,
            ManualStatus::Up => 1,
            ManualStatus::Pending => 2,
        }),
        _ => None,
    };
    let mut active =
        monitor.run == RunState::Active && !matches!(settings, MonitorSettings::Unsupported(_));
    let input = MonitorInput {
        name: notes.clip("monitor", &monitor.name, NAME_MAX_CHARS),
        description: notes.clip(
            "monitor description",
            &monitor.description,
            MONITOR_DESCRIPTION_MAX_CHARS,
        ),
        settings: settings.clone(),
        interval: monitor.interval,
        retry_interval: Some(monitor.retry_interval),
        max_retries: monitor.max_retries,
        resend_interval: monitor.resend_interval,
        timeout: Some(monitor.timeout),
        upside_down: monitor.upside_down,
        quorum: Quorum::Majority,
        tags: labels.clone(),
        group: None,
        probes: Vec::new(),
        channels: None,
    };
    // Settings v1per's form would refuse are kept as they are, paused, so nothing is lost.
    // Unsupported settings never validate and never run; their row is in the inventory.
    let checked = match settings {
        MonitorSettings::Unsupported(_) => Ok(()),
        _ => settings::validate(input.clone(), &HashSet::new()).map(drop),
    };
    if let Err(error) = checked {
        let fields: Vec<&String> = error
            .body()
            .errors
            .iter()
            .flatten()
            .map(|(f, _)| f)
            .collect();
        notes.add(format!(
            "monitor `{}` was imported paused: {fields:?} do not pass v1per's checks",
            monitor.name
        ));
        active = false;
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO monitors (node_id, name, description, type, settings, interval,
                               retry_interval, max_retries, resend_interval, timeout, upside_down,
                               quorum, active, tags, manual_status, revision, created_at,
                               updated_at)
         VALUES (?, ?, ?, '', '{}', ?, ?, ?, ?, ?, ?, 'majority', ?, ?, ?, 1, ?, ?)
         RETURNING id",
    )
    .bind(node.get())
    .bind(&input.name)
    .bind(&input.description)
    .bind(monitor.interval)
    .bind(monitor.retry_interval)
    .bind(monitor.max_retries)
    .bind(monitor.resend_interval)
    .bind(monitor.timeout)
    .bind(monitor.upside_down)
    .bind(active)
    .bind(serde_json::to_string(&labels)?)
    .bind(manual_status)
    .bind(now)
    .bind(now)
    .fetch_one(&mut *conn)
    .await?;
    let id = MonitorId::new(id);
    settings::store(conn, key, id, settings, settings::TokenSource::Kuma)
        .await
        .map_err(|error| anyhow::anyhow!("{}", error.body().message))?;
    Ok(id)
}

/// The protocol settings of a planned monitor. Anything v1per cannot run as Kuma ran it is
/// `Unsupported`, which never runs; its Kuma row is in the inventory.
fn settings_of(monitor: &MonitorPlan) -> MonitorSettings {
    let unsupported = |reason: &str| {
        MonitorSettings::Unsupported(UnsupportedSettings {
            kuma_type: monitor.kuma_type.clone(),
            reason: reason.to_owned(),
        })
    };
    if let RunState::PausedUnsupported { reason } = &monitor.run {
        return unsupported(reason);
    }
    match &monitor.kind {
        MonitorKind::Check(definition) => check_settings(definition, monitor)
            .unwrap_or_else(|| unsupported("authentication v1per does not support")),
        MonitorKind::Push { token } => MonitorSettings::Push(PushSettings {
            push_token: token.as_ref().map(|token| token.expose().to_owned()),
        }),
        MonitorKind::Group => MonitorSettings::Group,
        MonitorKind::Manual { .. } => MonitorSettings::Manual,
        MonitorKind::Unsupported => unsupported("monitor type not supported by v1per"),
    }
}

fn check_settings(
    definition: &checks::CheckDefinition,
    monitor: &MonitorPlan,
) -> Option<MonitorSettings> {
    Some(match &definition.check {
        checks::Check::Http(request) => {
            MonitorSettings::Http(http_settings(request, monitor.expiry_notification)?)
        }
        checks::Check::Keyword(keyword) => MonitorSettings::Keyword(KeywordSettings {
            request: http_settings(&keyword.request, monitor.expiry_notification)?,
            keyword: keyword.keyword.clone(),
            invert_keyword: keyword.invert_keyword,
        }),
        checks::Check::JsonQuery(query) => MonitorSettings::JsonQuery(JsonQuerySettings {
            request: http_settings(&query.request, monitor.expiry_notification)?,
            json_path: query.json_path.clone(),
            json_path_operator: json_path_operator(query.json_path_operator),
            expected_value: query.expected_value.clone(),
            retry_only_on_status_code_failure: monitor.retry_only_on_status_code_failure,
        }),
        checks::Check::Port(port) => MonitorSettings::Port(PortSettings {
            hostname: port.hostname.clone(),
            port: port.port,
        }),
        checks::Check::Ping(ping) => MonitorSettings::Ping(PingSettings {
            hostname: ping.hostname.clone(),
            ping_count: ping.ping_count,
            packet_size: ping.packet_size,
            ping_per_request_timeout: ping.ping_per_request_timeout,
        }),
        checks::Check::Dns(dns) => MonitorSettings::Dns(DnsSettings {
            hostname: dns.hostname.clone(),
            dns_resolve_type: dns_record_type(dns.dns_resolve_type),
            dns_resolve_server: dns.dns_resolve_server.clone(),
            port: dns.port,
        }),
    })
}

fn secret(value: &checks::Secret) -> SecretField {
    SecretField {
        value: Some(value.expose().to_owned()),
        clear: false,
        set: false,
    }
}

/// `None` for NTLM and mTLS, which v1per does not run.
fn http_settings(request: &checks::HttpRequest, expiry_notification: bool) -> Option<HttpSettings> {
    let auth = match &request.auth {
        checks::HttpAuth::None => HttpAuth::None,
        checks::HttpAuth::Basic {
            basic_auth_user,
            basic_auth_pass,
        } => HttpAuth::Basic {
            basic_auth_user: basic_auth_user.clone(),
            basic_auth_pass: secret(basic_auth_pass),
        },
        checks::HttpAuth::Bearer { bearer_token } => HttpAuth::Bearer {
            bearer_token: secret(bearer_token),
        },
        checks::HttpAuth::OAuth2ClientCredentials {
            oauth_token_url,
            oauth_client_id,
            oauth_client_secret,
            oauth_scopes,
            oauth_audience,
            oauth_auth_method,
        } => HttpAuth::OAuth2ClientCredentials {
            oauth_token_url: oauth_token_url.to_string(),
            oauth_client_id: oauth_client_id.clone(),
            oauth_client_secret: secret(oauth_client_secret),
            oauth_scopes: oauth_scopes.clone(),
            oauth_audience: oauth_audience.clone(),
            oauth_auth_method: match oauth_auth_method {
                checks::OAuthClientAuth::ClientSecretBasic => OAuthClientAuth::ClientSecretBasic,
                checks::OAuthClientAuth::ClientSecretPost => OAuthClientAuth::ClientSecretPost,
            },
        },
        checks::HttpAuth::Ntlm { .. } | checks::HttpAuth::Mtls { .. } => return None,
    };
    Some(HttpSettings {
        url: request.url.to_string(),
        method: request.method.clone(),
        // Kuma cannot mark a header secret, and they often carry API keys: all are sealed.
        headers: request
            .headers
            .iter()
            .map(|(name, value)| HttpHeader {
                name: name.clone(),
                value: None,
                secret: Some(secret(value)),
            })
            .collect(),
        body: request.body.clone(),
        http_body_encoding: match request.http_body_encoding {
            checks::HttpBodyEncoding::Json => HttpBodyEncoding::Json,
            checks::HttpBodyEncoding::Form => HttpBodyEncoding::Form,
            checks::HttpBodyEncoding::Xml => HttpBodyEncoding::Xml,
        },
        auth,
        accepted_statuscodes: request
            .accepted_statuscodes
            .iter()
            .map(|range| String::from(*range))
            .collect(),
        maxredirects: request.maxredirects,
        ignore_tls: request.ignore_tls,
        ip_family: request.ip_family.map(|family| match family {
            checks::IpFamily::Ipv4 => IpFamily::Ipv4,
            checks::IpFamily::Ipv6 => IpFamily::Ipv6,
        }),
        cache_bust: request.cache_bust,
        expiry_notification: expiry_notification && !request.ignore_tls,
    })
}

fn json_path_operator(operator: checks::JsonPathOperator) -> JsonPathOperator {
    use checks::JsonPathOperator as Op;
    match operator {
        Op::Equals => JsonPathOperator::Equals,
        Op::NotEquals => JsonPathOperator::NotEquals,
        Op::Less => JsonPathOperator::Less,
        Op::LessOrEqual => JsonPathOperator::LessOrEqual,
        Op::Greater => JsonPathOperator::Greater,
        Op::GreaterOrEqual => JsonPathOperator::GreaterOrEqual,
        Op::Contains => JsonPathOperator::Contains,
    }
}

fn dns_record_type(record: checks::DnsRecordType) -> DnsRecordType {
    use checks::DnsRecordType as Rr;
    match record {
        Rr::A => DnsRecordType::A,
        Rr::Aaaa => DnsRecordType::Aaaa,
        Rr::Cname => DnsRecordType::Cname,
        Rr::Mx => DnsRecordType::Mx,
        Rr::Ns => DnsRecordType::Ns,
        Rr::Txt => DnsRecordType::Txt,
        Rr::Srv => DnsRecordType::Srv,
        Rr::Ptr => DnsRecordType::Ptr,
        Rr::Caa => DnsRecordType::Caa,
        Rr::Soa => DnsRecordType::Soa,
    }
}

/// `maintenances` columns, as `incidents::maintenance` stores them.
struct StoredSchedule {
    strategy: &'static str,
    start_date: Option<String>,
    end_date: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    weekdays: String,
    days_of_month: String,
    timezone: Option<String>,
}

impl StoredSchedule {
    fn of(definition: &Definition) -> Self {
        let date = |date: &Option<chrono::NaiveDateTime>| {
            date.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        };
        let time = |time: &Option<chrono::NaiveTime>| time.map(|t| t.format("%H:%M").to_string());
        let days: Vec<Value> = definition
            .days_of_month
            .iter()
            .map(|day| match day {
                DayOfMonth::Day(day) => Value::from(*day),
                DayOfMonth::LastDay(n) => Value::from(format!("lastDay{n}")),
            })
            .collect();
        Self {
            strategy: match definition.strategy {
                Strategy::Manual => "manual",
                Strategy::Single => "single",
                Strategy::RecurringInterval => "recurring-interval",
                Strategy::RecurringWeekday => "recurring-weekday",
                Strategy::RecurringDayOfMonth => "recurring-day-of-month",
                Strategy::Cron => "cron",
            },
            start_date: date(&definition.start_date),
            end_date: date(&definition.end_date),
            start_time: time(&definition.start_time),
            end_time: time(&definition.end_time),
            weekdays: Value::from(definition.weekdays.clone()).to_string(),
            days_of_month: Value::Array(days).to_string(),
            timezone: match definition.timezone {
                Timezone::SameAsServer => None,
                Timezone::Named(zone) => Some(zone.name().to_owned()),
            },
        }
    }
}

async fn write_page(
    conn: &mut SqliteConnection,
    page: &StatusPagePlan,
    node: NodeId,
    monitors: &BTreeMap<i64, MonitorId>,
    timezone: &str,
    notes: &mut Notes,
    now: i64,
) -> anyhow::Result<i64> {
    let slug = page.slug.trim().to_lowercase();
    // A published page without password is public; an unpublished one is for grant holders.
    let password_hash = page
        .password
        .as_ref()
        .map(|password| identity::hash_password(password.expose()));
    let public = page.published && password_hash.is_none();
    let theme = match page.theme.as_str() {
        "light" | "dark" => page.theme.as_str(),
        _ => "auto",
    };
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO pages (node_id, slug, title, description, footer, theme, accent, timezone,
                            default_locale, locales, auto_incidents, grace_minutes, public,
                            password_hash, revision, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, 'indigo', ?, 'en', '[\"en\"]', 'draft', 0, ?, ?, 1, ?, ?)
         RETURNING id",
    )
    .bind(node.get())
    .bind(&slug)
    .bind(notes.clip("status page", &page.title, PAGE_TITLE_MAX))
    .bind(notes.clip(
        "status page description",
        page.description.as_deref().unwrap_or_default(),
        PAGE_DESCRIPTION_MAX_CHARS,
    ))
    .bind(notes.clip(
        "status page footer",
        page.footer_text.as_deref().unwrap_or_default(),
        PAGE_FOOTER_MAX_CHARS,
    ))
    .bind(theme)
    .bind(timezone)
    .bind(public)
    .bind(password_hash)
    .bind(now)
    .bind(now)
    .fetch_one(&mut *conn)
    .await?;

    let mut placed = BTreeSet::new();
    for (position, group) in page.groups.iter().enumerate() {
        let group_id: i64 = sqlx::query_scalar(
            "INSERT INTO page_groups (page_id, position, name, default_open)
             VALUES (?, ?, ?, 1) RETURNING id",
        )
        .bind(id)
        .bind(position as i64)
        .bind(notes.clip("status page group", &group.name, NAME_MAX_CHARS))
        .fetch_one(&mut *conn)
        .await?;
        let mut index = 0_i64;
        for component in &group.components {
            let Some(monitor) = monitors.get(&component.monitor.0) else {
                continue;
            };
            // A monitor shows once per page in v1per; Kuma allowed it in several groups.
            if !placed.insert(*monitor) {
                notes.add(format!(
                    "status page `{slug}` listed a monitor in several groups; it shows in the first"
                ));
                continue;
            }
            if component.send_url || component.custom_url.is_some() {
                notes.add(
                    "status page monitor links (Kuma's `send_url`, `custom_url`) are not shown",
                );
            }
            sqlx::query(
                "INSERT INTO page_components (page_id, group_id, position, monitor_id, internal)
                 VALUES (?, ?, ?, ?, 0)",
            )
            .bind(id)
            .bind(group_id)
            .bind(index)
            .bind(monitor.get())
            .execute(&mut *conn)
            .await?;
            index += 1;
        }
    }

    let mut claimed = 0;
    for domain in &page.domains {
        let Some(hostname) = normalize_hostname(domain) else {
            notes.add(format!(
                "status page `{slug}`: `{domain}` is not a hostname and was not claimed"
            ));
            continue;
        };
        if claimed == PAGE_HOSTNAMES_MAX {
            notes.add(format!(
                "status page `{slug}` claims at most {PAGE_HOSTNAMES_MAX} hostnames; the rest were dropped"
            ));
            break;
        }
        sqlx::query("INSERT INTO page_hostnames (hostname, page_id, created_at) VALUES (?, ?, ?)")
            .bind(&hostname)
            .bind(id)
            .bind(now)
            .execute(&mut *conn)
            .await?;
        claimed += 1;
    }
    if !page.icon.trim().is_empty() && page.icon != "/icon.svg" {
        notes.add("status page logos are files next to Kuma's database and were not imported");
    }
    Ok(id)
}

async fn write_user(
    conn: &mut SqliteConnection,
    key: &SecretKey,
    user: &UserPlan,
    node: NodeId,
    options: &Options,
    now: i64,
) -> anyhow::Result<(UserId, ImportedLogin)> {
    let email = login(&user.username, &options.login_domain);
    // An inactive Kuma user cannot sign in; a legacy hash cannot be checked.
    let kept = user.password_bcrypt.as_ref().filter(|_| user.active);
    let hash = match kept {
        Some(hash) => hash.expose().to_owned(),
        None => identity::hash_password(&identity::generate_password()),
    };
    let instance_admin = user.instance_admin && user.active;
    let id =
        identity::create_user(conn, &email, &user.username, &hash, instance_admin, now).await?;
    if user.active {
        sqlx::query("INSERT INTO grants (node_id, user_id, role, created_at) VALUES (?, ?, ?, ?)")
            .bind(node.get())
            .bind(id.as_str())
            .bind(IMPORTED_ROLE)
            .bind(now)
            .execute(&mut *conn)
            .await?;
    }
    // Kuma uses the secret's ASCII bytes as the HMAC key, so they are the shared secret.
    if let Some(secret) = &user.totp_secret {
        let sealed = key.seal(
            &identity::two_factor::context(&id),
            secret.expose().as_bytes(),
        );
        sqlx::query("INSERT INTO two_factor (user_id, secret, confirmed_at) VALUES (?, ?, ?)")
            .bind(id.as_str())
            .bind(sealed)
            .bind(now)
            .execute(&mut *conn)
            .await?;
    }
    let login = ImportedLogin {
        kuma_username: user.username.clone(),
        email,
        instance_admin,
        two_factor: user.totp_secret.is_some(),
        password: if kept.is_some() {
            ImportedPassword::Kept
        } else {
            ImportedPassword::Reset
        },
    };
    Ok((id, login))
}

/// Kuma settings fill instance settings still at their defaults; an instance that has its own
/// keeps them.
async fn write_settings(
    conn: &mut SqliteConnection,
    plan: &ImportPlan,
    notes: &mut Notes,
    now: i64,
) -> sqlx::Result<()> {
    let settings = &plan.settings;
    let mut kept = Vec::new();
    if let Some(url) = &settings.primary_base_url {
        let set = sqlx::query(
            "UPDATE instance_settings SET public_url = ?, updated_at = ?
             WHERE id = 1 AND public_url IS NULL",
        )
        .bind(url.trim_end_matches('/'))
        .bind(now)
        .execute(&mut *conn)
        .await?;
        if set.rows_affected() == 0 {
            kept.push("public URL");
        }
    }
    if let Some(zone) = settings.timezone {
        let set = sqlx::query(
            "UPDATE instance_settings SET timezone = ?, updated_at = ?
             WHERE id = 1 AND timezone = 'UTC'",
        )
        .bind(zone.name())
        .bind(now)
        .execute(&mut *conn)
        .await?;
        if set.rows_affected() == 0 && zone.name() != "UTC" {
            kept.push("timezone");
        }
    }
    if let Some(days) = &settings.tls_expiry_notify_days {
        let set = sqlx::query(
            "UPDATE instance_settings SET tls_expiry_notify_days = ?, updated_at = ?
             WHERE id = 1 AND tls_expiry_notify_days = '[7,14,21]'",
        )
        .bind(Value::from(days.clone()).to_string())
        .bind(now)
        .execute(&mut *conn)
        .await?;
        if set.rows_affected() == 0 {
            kept.push("certificate expiry days");
        }
    }
    if !kept.is_empty() {
        notes.add(format!(
            "the instance keeps its own {}; Kuma's were not applied",
            kept.join(", ")
        ));
    }
    Ok(())
}
