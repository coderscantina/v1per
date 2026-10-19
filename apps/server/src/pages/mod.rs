//! Status pages (milestone 09, decision 0004): `pages.*`, incident publication, and the public
//! routes at `/s/{slug}` and on claimed hostnames.
//!
//! A page shows monitors and static components from its node's subtree. Everything its
//! snapshot depends on records `page.updated`, `page.status_changed` or
//! `page.incident_changed` on the page's own `page:<id>` topic in the transaction that changed
//! it ([`touch`], [`touch_in`]), so the public stream and the snapshot cache follow one log.
//! Admin changes also go to the `pages:<id>` topic of the page's node and every ancestor.

mod access;
mod badge;
mod logo;
mod mail;
mod public;
pub(crate) mod publish;
mod snapshot;
mod text;

pub(crate) use logo::upload as upload_logo;
pub(crate) use public::{host_routing, hostname_router, instance_routes};
pub use publish::run as run_auto_publish;
pub use snapshot::SnapshotCache;

use std::collections::{HashMap, HashSet};

use chrono_tz::Tz;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::{
    ComponentId, IncidentId, MaintenanceId, MonitorId, NodeId, PageComponentId, PageGroupId,
    PageId, ShareTokenId,
};
use v1per_protocol::limits::{
    NAME_MAX_CHARS, PAGE_ALLOWED_EMAILS_MAX, PAGE_COMPONENTS_MAX, PAGE_DESCRIPTION_MAX_CHARS,
    PAGE_FOOTER_MAX_CHARS, PAGE_GRACE_MAX_MINUTES, PAGE_GROUPS_MAX, PAGE_HOSTNAMES_MAX, PAGE_PATH,
    PAGE_SHARE_TOKENS_MAX, PAGE_SLUG_MAX_CHARS, PASSWORD_MAX_CHARS,
};
use v1per_protocol::operations::incidents::Target;
use v1per_protocol::operations::monitors::SecretField;
use v1per_protocol::operations::pages::{
    Page, PageAccess, PageAccessMode, PageArgs, PageComponent, PageDeleted, PageDetail, PageGroup,
    PageList, PageLogo, PagePreview, PageSettings, PageShareToken, PageSummary, PagesCreateArgs,
    PagesCreateShareTokenArgs, PagesHandlers, PagesHostnameArgs, PagesListArgs, PagesPreviewArgs,
    PagesUpdateAccessArgs, PagesUpdateArgs, PagesUpdateLayoutArgs, ShareTokenArgs,
    ShareTokenCreated, ShareTokenRevoked,
};
use v1per_protocol::registry::Snapshot;
use v1per_protocol::status_page::{Accent, Locale, PageTheme};

use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::identity::{self, validate_email};
use crate::incidents::{check_targets, record_tree_ctx, text};
use crate::instance;
use crate::state::AppState;

const TITLE_MAX: usize = 100;

/// Text form of a unit enum, as stored.
fn enum_text<T: serde::Serialize>(value: T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(text)) => text,
        _ => unreachable!("unit enums serialize as strings"),
    }
}

fn enum_from<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, ApiError> {
    serde_json::from_value(Value::String(text.to_owned())).map_err(ApiError::internal)
}

/// A `pages` row without the logo bytes.
#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct PageRow {
    pub id: i64,
    pub node_id: i64,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub footer: String,
    pub theme: String,
    pub accent: String,
    pub timezone: String,
    pub default_locale: String,
    pub locales: String,
    pub auto_incidents: String,
    pub grace_minutes: i64,
    pub public: bool,
    pub password_hash: Option<String>,
    pub allowed_emails: String,
    pub allowed_domains: String,
    pub logo_type: Option<String>,
    pub logo_size: Option<i64>,
    pub logo_updated_at: Option<i64>,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

macro_rules! select_page {
    () => {
        "SELECT id, node_id, slug, title, description, footer, theme, accent, timezone,
                default_locale, locales, auto_incidents, grace_minutes, public, password_hash,
                allowed_emails, allowed_domains, logo_type, length(logo) AS logo_size,
                logo_updated_at, revision, created_at, updated_at
         FROM pages"
    };
}
pub(crate) use select_page;

impl PageRow {
    pub fn page(&self) -> PageId {
        PageId::new(self.id)
    }

    pub fn node(&self) -> NodeId {
        NodeId::new(self.node_id)
    }

    pub fn zone(&self) -> Tz {
        self.timezone.parse().unwrap_or(Tz::UTC)
    }

    pub fn default_locale(&self) -> Locale {
        Locale::parse(&self.default_locale).unwrap_or_default()
    }

    /// Enabled locales, the default first.
    pub fn locales(&self) -> Vec<Locale> {
        let stored: Vec<Locale> = serde_json::from_str(&self.locales).unwrap_or_default();
        let default = self.default_locale();
        std::iter::once(default)
            .chain(stored.into_iter().filter(|locale| *locale != default))
            .collect()
    }

    pub fn theme(&self) -> PageTheme {
        enum_from(&self.theme).unwrap_or_default()
    }

    pub fn accent(&self) -> Accent {
        enum_from(&self.accent).unwrap_or_default()
    }

    pub fn emails(&self) -> Vec<String> {
        serde_json::from_str(&self.allowed_emails).unwrap_or_default()
    }

    pub fn domains(&self) -> Vec<String> {
        serde_json::from_str(&self.allowed_domains).unwrap_or_default()
    }

    /// Whether `email` (normalized) may request a sign-in link.
    pub fn allows_email(&self, email: &str) -> bool {
        let domain = email.rsplit_once('@').map(|(_, domain)| domain);
        self.emails().iter().any(|allowed| allowed == email)
            || domain.is_some_and(|domain| self.domains().iter().any(|allowed| allowed == domain))
    }

    pub fn email_access(&self) -> bool {
        !self.emails().is_empty() || !self.domains().is_empty()
    }

    /// `/s/<slug>`.
    pub fn path(&self) -> String {
        format!("{PAGE_PATH}{}", self.slug)
    }
}

pub(crate) async fn load(conn: &mut SqliteConnection, page: PageId) -> Result<PageRow, ApiError> {
    sqlx::query_as(concat!(select_page!(), " WHERE id = ?"))
        .bind(page.get())
        .fetch_optional(conn)
        .await?
        .ok_or_else(ApiError::not_found)
}

/// What changed, for the pages that show it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Change {
    Monitor(MonitorId),
    Component(ComponentId),
    /// Published incidents only reach the pages they are published to.
    Incident(IncidentId),
    /// The pages showing a monitor the maintenance covers, directly or through a group.
    Maintenance(MaintenanceId),
}

impl Change {
    fn kind(self) -> &'static str {
        match self {
            Change::Incident(_) => "page.incident_changed",
            _ => "page.status_changed",
        }
    }

    async fn pages(self, conn: &mut SqliteConnection) -> sqlx::Result<Vec<i64>> {
        match self {
            Change::Monitor(monitor) => {
                sqlx::query_scalar(
                    "SELECT DISTINCT page_id FROM page_components WHERE monitor_id = ?",
                )
                .bind(monitor.get())
                .fetch_all(conn)
                .await
            }
            Change::Component(component) => {
                sqlx::query_scalar(
                    "SELECT DISTINCT page_id FROM page_components WHERE component_id = ?",
                )
                .bind(component.get())
                .fetch_all(conn)
                .await
            }
            Change::Incident(incident) => {
                sqlx::query_scalar(
                    "SELECT page_id FROM incident_publications WHERE incident_id = ?",
                )
                .bind(incident.get())
                .fetch_all(conn)
                .await
            }
            Change::Maintenance(maintenance) => {
                sqlx::query_scalar(
                    "WITH RECURSIVE covered(id) AS (
                         SELECT monitor_id FROM maintenance_monitors WHERE maintenance_id = ?1
                         UNION
                         SELECT m.id FROM monitors m JOIN covered c ON m.group_id = c.id
                     )
                     SELECT DISTINCT page_id FROM page_components
                     WHERE monitor_id IN (SELECT id FROM covered)",
                )
                .bind(maintenance.get())
                .fetch_all(conn)
                .await
            }
        }
    }
}

/// Records `change` on the topic of every page it shows on, outside a command, and returns
/// the topics to wake after commit.
pub(crate) async fn touch(
    conn: &mut SqliteConnection,
    change: Change,
    now: i64,
) -> sqlx::Result<Vec<Topic>> {
    let mut topics = Vec::new();
    for page in change.pages(conn).await? {
        let topic = Topic::page(PageId::new(page));
        crate::events::record(conn, &topic, change.kind(), &json!({}), now).await?;
        topics.push(topic);
    }
    Ok(topics)
}

/// [`touch`] inside a command.
pub(crate) async fn touch_in(ctx: &mut CommandCtx, change: Change) -> Result<(), ApiError> {
    for page in change.pages(ctx.conn()).await? {
        ctx.record_also(&Topic::page(PageId::new(page)), change.kind(), json!({}))
            .await?;
    }
    Ok(())
}

/// After a node move: takes off every page component whose target left the page's subtree.
pub(crate) async fn prune_moved(ctx: &mut CommandCtx) -> Result<(), ApiError> {
    let pages: Vec<i64> = sqlx::query_scalar(
        "WITH RECURSIVE up(start, id) AS (
             SELECT id, id FROM nodes
             UNION ALL
             SELECT up.start, n.parent_id FROM nodes n JOIN up ON n.id = up.id
             WHERE n.parent_id IS NOT NULL
         )
         DELETE FROM page_components WHERE id IN (
             SELECT pc.id FROM page_components pc
             JOIN pages p ON p.id = pc.page_id
             LEFT JOIN monitors m ON m.id = pc.monitor_id
             LEFT JOIN components c ON c.id = pc.component_id
             WHERE NOT EXISTS (SELECT 1 FROM up
                               WHERE up.start = coalesce(m.node_id, c.node_id)
                                 AND up.id = p.node_id)
         )
         RETURNING page_id",
    )
    .fetch_all(ctx.conn())
    .await?;
    let pages: HashSet<i64> = pages.into_iter().collect();
    for page in pages {
        ctx.record_also(&Topic::page(PageId::new(page)), "page.updated", json!({}))
            .await?;
    }
    Ok(())
}

/// Records a person's change to a page: `kind` on the `pages:<id>` topics, `page.updated` on
/// the page's own topic.
async fn changed(ctx: &mut CommandCtx, page: &PageRow, kind: &str) -> Result<(), ApiError> {
    ctx.record_also(&Topic::page(page.page()), "page.updated", json!({}))
        .await?;
    record_tree_ctx(
        ctx,
        Topic::pages,
        page.node(),
        kind,
        json!({ "page": page.page() }),
    )
    .await
}

/// Bumps the page's revision after a person's change.
async fn bump(ctx: &mut CommandCtx, page: PageId) -> Result<(), ApiError> {
    let now = ctx.now();
    sqlx::query("UPDATE pages SET revision = revision + 1, updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(page.get())
        .execute(ctx.conn())
        .await?;
    Ok(())
}

/// The page and the caller's scope on its node with `capability`, then the revision check.
async fn locate(
    ctx: &mut CommandCtx,
    page: PageId,
    capability: Capability,
    expected_revision: Option<i64>,
) -> Result<PageRow, ApiError> {
    let row = load(ctx.conn(), page).await?;
    ctx.node_scope(row.node(), capability).await?;
    if let Some(expected) = expected_revision
        && expected != row.revision
    {
        return Err(ApiError::stale_revision(row.revision));
    }
    Ok(row)
}

pub(crate) fn valid_slug(slug: &str) -> bool {
    let bytes = slug.as_bytes();
    !bytes.is_empty()
        && slug.chars().count() <= PAGE_SLUG_MAX_CHARS
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
}

/// A DNS name of at least two labels, lowercase, without a trailing dot.
pub(crate) fn normalize_hostname(hostname: &str) -> Option<String> {
    let name = hostname.trim().trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = name.split('.').collect();
    let valid = name.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        });
    valid.then_some(name)
}

/// The host of an `http(s)://host[:port]` origin.
pub(crate) fn origin_host(origin: &str) -> Option<String> {
    let rest = origin.split_once("://")?.1;
    let host = rest.split('/').next()?;
    let host = host.rsplit_once(':').map_or(host, |(host, _)| host);
    Some(host.to_ascii_lowercase())
}

/// Settings checked and in stored form.
struct ValidSettings {
    slug: String,
    title: String,
    description: String,
    footer: String,
    timezone: String,
    locales: Vec<Locale>,
}

async fn validate_settings(
    conn: &mut SqliteConnection,
    settings: &PageSettings,
    page: Option<PageId>,
) -> Result<ValidSettings, ApiError> {
    let slug = settings.slug.trim().to_owned();
    if !valid_slug(&slug) {
        return Err(FieldErrors::one(
            "slug",
            FieldErrorCode::InvalidFormat,
            Some(PAGE_SLUG_MAX_CHARS as u32),
        ));
    }
    let taken: Option<i64> = sqlx::query_scalar("SELECT id FROM pages WHERE slug = ?")
        .bind(&slug)
        .fetch_optional(&mut *conn)
        .await?;
    if taken.is_some_and(|id| Some(PageId::new(id)) != page) {
        return Err(FieldErrors::one("slug", FieldErrorCode::InvalidValue, None));
    }
    let title = text("title", &settings.title, true, TITLE_MAX)?;
    let description = text(
        "description",
        &settings.description,
        false,
        PAGE_DESCRIPTION_MAX_CHARS,
    )?;
    let footer = text("footer", &settings.footer, false, PAGE_FOOTER_MAX_CHARS)?;
    let timezone = match &settings.timezone {
        Some(zone) if zone.parse::<Tz>().is_ok() => zone.clone(),
        Some(_) => {
            return Err(FieldErrors::one(
                "timezone",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        None => instance::timezone(conn).await?.name().to_owned(),
    };
    if settings.grace_minutes > PAGE_GRACE_MAX_MINUTES {
        return Err(FieldErrors::one(
            "grace_minutes",
            FieldErrorCode::InvalidValue,
            Some(PAGE_GRACE_MAX_MINUTES),
        ));
    }
    let mut locales = vec![settings.default_locale];
    for locale in Locale::ALL {
        if settings.locales.contains(&locale) && !locales.contains(&locale) {
            locales.push(locale);
        }
    }
    Ok(ValidSettings {
        slug,
        title,
        description,
        footer,
        timezone,
        locales,
    })
}

/// Writes the settings of a page, or of a new page on `node` with the other columns at their
/// defaults: restricted to grant holders until someone opens it, no password, no allowed
/// addresses.
async fn store_settings(
    ctx: &mut CommandCtx,
    page: Result<PageId, NodeId>,
    settings: &PageSettings,
    valid: ValidSettings,
) -> Result<PageId, ApiError> {
    let now = ctx.now();
    let sql = match page {
        Ok(_) => {
            "UPDATE pages SET slug = ?1, title = ?2, description = ?3, footer = ?4, theme = ?5,
                              accent = ?6, timezone = ?7, default_locale = ?8, locales = ?9,
                              auto_incidents = ?10, grace_minutes = ?11, updated_at = ?13
             WHERE id = ?12 RETURNING id"
        }
        Err(_) => {
            "INSERT INTO pages (slug, title, description, footer, theme, accent, timezone,
                                default_locale, locales, auto_incidents, grace_minutes, node_id,
                                public, revision, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, 1, ?13, ?13)
             RETURNING id"
        }
    };
    let id: i64 = sqlx::query_scalar(sql)
        .bind(&valid.slug)
        .bind(&valid.title)
        .bind(&valid.description)
        .bind(&valid.footer)
        .bind(enum_text(settings.theme))
        .bind(enum_text(settings.accent))
        .bind(&valid.timezone)
        .bind(enum_text(settings.default_locale))
        .bind(serde_json::to_string(&valid.locales).map_err(ApiError::internal)?)
        .bind(enum_text(settings.auto_incidents))
        .bind(i64::from(settings.grace_minutes))
        .bind(page.map_or_else(NodeId::get, PageId::get))
        .bind(now)
        .fetch_one(ctx.conn())
        .await?;
    Ok(PageId::new(id))
}

/// The page as its managers see it.
async fn detail(conn: &mut SqliteConnection, page: PageId) -> Result<PageDetail, ApiError> {
    let row = load(conn, page).await?;
    let hostnames: Vec<String> = sqlx::query_scalar(
        "SELECT hostname FROM page_hostnames WHERE page_id = ? ORDER BY hostname",
    )
    .bind(row.id)
    .fetch_all(&mut *conn)
    .await?;
    let groups: Vec<(i64, String, bool)> = sqlx::query_as(
        "SELECT id, name, default_open FROM page_groups WHERE page_id = ? ORDER BY position",
    )
    .bind(row.id)
    .fetch_all(&mut *conn)
    .await?;
    #[derive(sqlx::FromRow)]
    struct Placed {
        id: i64,
        group_id: i64,
        monitor_id: Option<i64>,
        component_id: Option<i64>,
        display_name: Option<String>,
        internal: bool,
        source: String,
    }
    let components: Vec<Placed> = sqlx::query_as(
        "SELECT pc.id, pc.group_id, pc.monitor_id, pc.component_id, pc.display_name,
                    pc.internal, coalesce(m.name, c.name, '') AS source
             FROM page_components pc
             LEFT JOIN monitors m ON m.id = pc.monitor_id
             LEFT JOIN components c ON c.id = pc.component_id
             WHERE pc.page_id = ? ORDER BY pc.position",
    )
    .bind(row.id)
    .fetch_all(&mut *conn)
    .await?;
    let groups = groups
        .into_iter()
        .map(|(id, name, default_open)| PageGroup {
            id: PageGroupId::new(id),
            name,
            default_open,
            components: components
                .iter()
                .filter(|placed| placed.group_id == id)
                .map(|placed| PageComponent {
                    id: PageComponentId::new(placed.id),
                    target: match placed.monitor_id {
                        Some(monitor) => Target::Monitor {
                            monitor: MonitorId::new(monitor),
                        },
                        None => Target::Component {
                            component: ComponentId::new(placed.component_id.unwrap_or_default()),
                        },
                    },
                    name: placed
                        .display_name
                        .clone()
                        .unwrap_or_else(|| placed.source.clone()),
                    display_name: placed.display_name.clone(),
                    internal: placed.internal,
                })
                .collect(),
        })
        .collect();
    #[derive(sqlx::FromRow)]
    struct Token {
        id: i64,
        name: String,
        expires_at: Option<i64>,
        created_by: Option<String>,
        created_at: i64,
    }
    let tokens: Vec<Token> = sqlx::query_as(
        "SELECT t.id, t.name, t.expires_at, u.name AS created_by, t.created_at
         FROM page_share_tokens t LEFT JOIN users u ON u.id = t.created_by
         WHERE t.page_id = ? ORDER BY t.id",
    )
    .bind(row.id)
    .fetch_all(&mut *conn)
    .await?;
    let smtp_configured: bool = sqlx::query_scalar(
        "SELECT smtp_host IS NOT NULL AND smtp_from IS NOT NULL FROM instance_settings WHERE id = 1",
    )
    .fetch_one(&mut *conn)
    .await?;
    let path = row.path();
    Ok(PageDetail {
        page: Page {
            id: row.page(),
            node: row.node(),
            settings: PageSettings {
                slug: row.slug.clone(),
                title: row.title.clone(),
                description: row.description.clone(),
                footer: row.footer.clone(),
                theme: row.theme(),
                accent: row.accent(),
                timezone: Some(row.timezone.clone()),
                default_locale: row.default_locale(),
                locales: row.locales(),
                auto_incidents: enum_from(&row.auto_incidents)?,
                grace_minutes: row.grace_minutes as u32,
            },
            access: PageAccess {
                public: row.public,
                password: SecretField {
                    set: row.password_hash.is_some(),
                    ..SecretField::default()
                },
                emails: row.emails(),
                domains: row.domains(),
            },
            hostnames,
            logo: row.logo_type.clone().map(|content_type| PageLogo {
                content_type,
                size: row.logo_size.unwrap_or_default() as u32,
                url: format!("{path}{}", logo::path(&row).unwrap_or_default()),
                updated_at: row.logo_updated_at.unwrap_or_default(),
            }),
            path,
            revision: row.revision,
            created_at: row.created_at,
            updated_at: row.updated_at,
        },
        groups,
        share_tokens: tokens
            .into_iter()
            .map(|token| PageShareToken {
                id: ShareTokenId::new(token.id),
                name: token.name,
                expires_at: token.expires_at,
                created_by: token.created_by,
                created_at: token.created_at,
            })
            .collect(),
        smtp_configured,
    })
}

/// Allowed addresses and domains, normalized, each once.
fn validate_allowed(access: &PageAccess) -> Result<(Vec<String>, Vec<String>), ApiError> {
    let mut errors = FieldErrors::default();
    let mut emails: Vec<String> = Vec::new();
    for email in &access.emails {
        match validate_email("emails", email) {
            Ok(email) if !emails.contains(&email) => emails.push(email),
            Ok(_) => {}
            Err(_) => errors.add("emails", FieldErrorCode::InvalidFormat, None),
        }
    }
    let mut domains: Vec<String> = Vec::new();
    for domain in &access.domains {
        match normalize_hostname(domain.trim().trim_start_matches('@')) {
            Some(domain) if !domains.contains(&domain) => domains.push(domain),
            Some(_) => {}
            None => errors.add("domains", FieldErrorCode::InvalidFormat, None),
        }
    }
    if emails.len() + domains.len() > PAGE_ALLOWED_EMAILS_MAX {
        errors.add(
            "emails",
            FieldErrorCode::TooMany,
            Some(PAGE_ALLOWED_EMAILS_MAX as u32),
        );
    }
    errors.finish()?;
    Ok((emails, domains))
}

impl PagesHandlers for AppState {
    async fn pages_create(
        &self,
        ctx: &mut CommandCtx,
        args: PagesCreateArgs,
    ) -> Result<PageDetail, ApiError> {
        ctx.node_scope(args.node, Capability::PagesManage).await?;
        let valid = validate_settings(ctx.conn(), &args.settings, None).await?;
        let page = store_settings(ctx, Err(args.node), &args.settings, valid).await?;
        let row = load(ctx.conn(), page).await?;
        changed(ctx, &row, "page.created").await?;
        detail(ctx.conn(), page).await
    }

    async fn pages_update(
        &self,
        ctx: &mut CommandCtx,
        args: PagesUpdateArgs,
    ) -> Result<PageDetail, ApiError> {
        let row = locate(
            ctx,
            args.page,
            Capability::PagesManage,
            args.expected_revision,
        )
        .await?;
        let valid = validate_settings(ctx.conn(), &args.settings, Some(args.page)).await?;
        store_settings(ctx, Ok(args.page), &args.settings, valid).await?;
        bump(ctx, args.page).await?;
        changed(ctx, &row, "page.updated").await?;
        detail(ctx.conn(), args.page).await
    }

    async fn pages_update_access(
        &self,
        ctx: &mut CommandCtx,
        args: PagesUpdateAccessArgs,
    ) -> Result<PageDetail, ApiError> {
        let row = locate(
            ctx,
            args.page,
            Capability::PagesManage,
            args.expected_revision,
        )
        .await?;
        let (emails, domains) = validate_allowed(&args.access)?;
        let password = &args.access.password;
        let hash = match (&password.value, password.clear) {
            (Some(_), true) => {
                return Err(FieldErrors::one(
                    "password",
                    FieldErrorCode::InvalidValue,
                    None,
                ));
            }
            (Some(value), false) => {
                if value.is_empty() || value.chars().count() > PASSWORD_MAX_CHARS {
                    return Err(FieldErrors::one(
                        "password",
                        FieldErrorCode::InvalidValue,
                        Some(PASSWORD_MAX_CHARS as u32),
                    ));
                }
                Some(Some(identity::hash_password_blocking(value.clone()).await?))
            }
            (None, true) => Some(None),
            (None, false) => None,
        };
        if let Some(hash) = &hash {
            sqlx::query("UPDATE pages SET password_hash = ? WHERE id = ?")
                .bind(hash)
                .bind(row.id)
                .execute(ctx.conn())
                .await?;
            // Whoever entered the old password has to enter the new one.
            sqlx::query("DELETE FROM page_sessions WHERE page_id = ? AND kind = 'password'")
                .bind(row.id)
                .execute(ctx.conn())
                .await?;
        }
        sqlx::query(
            "UPDATE pages SET public = ?, allowed_emails = ?, allowed_domains = ? WHERE id = ?",
        )
        .bind(args.access.public)
        .bind(serde_json::to_string(&emails).map_err(ApiError::internal)?)
        .bind(serde_json::to_string(&domains).map_err(ApiError::internal)?)
        .bind(row.id)
        .execute(ctx.conn())
        .await?;
        bump(ctx, args.page).await?;
        changed(ctx, &row, "page.updated").await?;
        detail(ctx.conn(), args.page).await
    }

    async fn pages_update_layout(
        &self,
        ctx: &mut CommandCtx,
        args: PagesUpdateLayoutArgs,
    ) -> Result<PageDetail, ApiError> {
        let row = locate(
            ctx,
            args.page,
            Capability::PagesManage,
            Some(args.expected_revision),
        )
        .await?;
        let mut errors = FieldErrors::default();
        if args.groups.len() > PAGE_GROUPS_MAX {
            errors.add(
                "groups",
                FieldErrorCode::TooMany,
                Some(PAGE_GROUPS_MAX as u32),
            );
        }
        let targets: Vec<Target> = args
            .groups
            .iter()
            .flat_map(|group| group.components.iter().map(|component| component.target))
            .collect();
        if targets.len() > PAGE_COMPONENTS_MAX {
            errors.add(
                "groups",
                FieldErrorCode::TooMany,
                Some(PAGE_COMPONENTS_MAX as u32),
            );
        }
        let existing: Vec<i64> = sqlx::query_scalar("SELECT id FROM page_groups WHERE page_id = ?")
            .bind(row.id)
            .fetch_all(ctx.conn())
            .await?;
        for group in &args.groups {
            if group.name.trim().chars().count() > NAME_MAX_CHARS {
                errors.add(
                    "groups",
                    FieldErrorCode::TooLong,
                    Some(NAME_MAX_CHARS as u32),
                );
            }
            let repeated = args
                .groups
                .iter()
                .filter(|other| other.id.is_some() && other.id == group.id)
                .count()
                > 1;
            if repeated || group.id.is_some_and(|id| !existing.contains(&id.get())) {
                errors.add("groups", FieldErrorCode::InvalidValue, None);
            }
            for component in &group.components {
                let name = component.display_name.as_deref().map(str::trim);
                if name.is_some_and(|name| name.chars().count() > NAME_MAX_CHARS) {
                    errors.add(
                        "groups",
                        FieldErrorCode::TooLong,
                        Some(NAME_MAX_CHARS as u32),
                    );
                }
            }
        }
        errors.finish()?;
        check_targets(ctx.conn(), row.node(), "groups", &targets).await?;

        let mut kept_groups = Vec::new();
        let mut kept_components = Vec::new();
        for (position, group) in args.groups.iter().enumerate() {
            let name = group.name.trim();
            let group_id: i64 = match group.id {
                Some(id) => {
                    sqlx::query(
                        "UPDATE page_groups SET position = ?, name = ?, default_open = ?
                         WHERE id = ?",
                    )
                    .bind(position as i64)
                    .bind(name)
                    .bind(group.default_open)
                    .bind(id.get())
                    .execute(ctx.conn())
                    .await?;
                    id.get()
                }
                None => {
                    sqlx::query_scalar(
                        "INSERT INTO page_groups (page_id, position, name, default_open)
                         VALUES (?, ?, ?, ?) RETURNING id",
                    )
                    .bind(row.id)
                    .bind(position as i64)
                    .bind(name)
                    .bind(group.default_open)
                    .fetch_one(ctx.conn())
                    .await?
                }
            };
            kept_groups.push(group_id);
            for (index, component) in group.components.iter().enumerate() {
                let (monitor, static_component) = match component.target {
                    Target::Monitor { monitor } => (Some(monitor.get()), None),
                    Target::Component { component } => (None, Some(component.get())),
                };
                let display_name = component
                    .display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty());
                let current: Option<i64> = sqlx::query_scalar(
                    "SELECT id FROM page_components
                     WHERE page_id = ? AND monitor_id IS ? AND component_id IS ?",
                )
                .bind(row.id)
                .bind(monitor)
                .bind(static_component)
                .fetch_optional(ctx.conn())
                .await?;
                let id: i64 =
                    match current {
                        Some(id) => {
                            sqlx::query(
                                "UPDATE page_components
                             SET group_id = ?, position = ?, display_name = ?, internal = ?
                             WHERE id = ?",
                            )
                            .bind(group_id)
                            .bind(index as i64)
                            .bind(display_name)
                            .bind(component.internal)
                            .bind(id)
                            .execute(ctx.conn())
                            .await?;
                            id
                        }
                        None => sqlx::query_scalar(
                            "INSERT INTO page_components (page_id, group_id, position, monitor_id,
                                                          component_id, display_name, internal)
                             VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
                        )
                        .bind(row.id)
                        .bind(group_id)
                        .bind(index as i64)
                        .bind(monitor)
                        .bind(static_component)
                        .bind(display_name)
                        .bind(component.internal)
                        .fetch_one(ctx.conn())
                        .await?,
                    };
                kept_components.push(id);
            }
        }
        sqlx::query(
            "DELETE FROM page_components
             WHERE page_id = ?1 AND id NOT IN (SELECT value FROM json_each(?2))",
        )
        .bind(row.id)
        .bind(serde_json::to_string(&kept_components).map_err(ApiError::internal)?)
        .execute(ctx.conn())
        .await?;
        sqlx::query(
            "DELETE FROM page_groups
             WHERE page_id = ?1 AND id NOT IN (SELECT value FROM json_each(?2))",
        )
        .bind(row.id)
        .bind(serde_json::to_string(&kept_groups).map_err(ApiError::internal)?)
        .execute(ctx.conn())
        .await?;
        bump(ctx, args.page).await?;
        changed(ctx, &row, "page.updated").await?;
        detail(ctx.conn(), args.page).await
    }

    async fn pages_claim_hostname(
        &self,
        ctx: &mut CommandCtx,
        args: PagesHostnameArgs,
    ) -> Result<PageDetail, ApiError> {
        let row = locate(ctx, args.page, Capability::PagesManage, None).await?;
        let invalid = |code| FieldErrors::one("hostname", code, None);
        let hostname =
            normalize_hostname(&args.hostname).ok_or(invalid(FieldErrorCode::InvalidFormat))?;
        let settings = instance::effective(ctx.conn(), &self.config).await?;
        let own = settings.public_url.as_deref().and_then(origin_host);
        if own.as_deref() == Some(hostname.as_str()) {
            return Err(invalid(FieldErrorCode::InvalidValue));
        }
        let claimed: Option<i64> =
            sqlx::query_scalar("SELECT page_id FROM page_hostnames WHERE hostname = ?")
                .bind(&hostname)
                .fetch_optional(ctx.conn())
                .await?;
        match claimed {
            Some(page) if page == row.id => return detail(ctx.conn(), args.page).await,
            Some(_) => return Err(invalid(FieldErrorCode::InvalidValue)),
            None => {}
        }
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM page_hostnames WHERE page_id = ?")
                .bind(row.id)
                .fetch_one(ctx.conn())
                .await?;
        if count as usize >= PAGE_HOSTNAMES_MAX {
            return Err(FieldErrors::one(
                "hostname",
                FieldErrorCode::TooMany,
                Some(PAGE_HOSTNAMES_MAX as u32),
            ));
        }
        sqlx::query("INSERT INTO page_hostnames (hostname, page_id, created_at) VALUES (?, ?, ?)")
            .bind(&hostname)
            .bind(row.id)
            .bind(ctx.now())
            .execute(ctx.conn())
            .await?;
        bump(ctx, args.page).await?;
        changed(ctx, &row, "page.updated").await?;
        detail(ctx.conn(), args.page).await
    }

    async fn pages_release_hostname(
        &self,
        ctx: &mut CommandCtx,
        args: PagesHostnameArgs,
    ) -> Result<PageDetail, ApiError> {
        let row = locate(ctx, args.page, Capability::PagesManage, None).await?;
        let hostname = args
            .hostname
            .trim()
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let released = sqlx::query("DELETE FROM page_hostnames WHERE hostname = ? AND page_id = ?")
            .bind(&hostname)
            .bind(row.id)
            .execute(ctx.conn())
            .await?
            .rows_affected();
        if released > 0 {
            bump(ctx, args.page).await?;
            changed(ctx, &row, "page.updated").await?;
        }
        detail(ctx.conn(), args.page).await
    }

    async fn pages_remove_logo(
        &self,
        ctx: &mut CommandCtx,
        args: PageArgs,
    ) -> Result<PageDetail, ApiError> {
        let row = locate(ctx, args.page, Capability::PagesManage, None).await?;
        if row.logo_type.is_some() {
            sqlx::query(
                "UPDATE pages SET logo = NULL, logo_type = NULL, logo_updated_at = NULL
                 WHERE id = ?",
            )
            .bind(row.id)
            .execute(ctx.conn())
            .await?;
            bump(ctx, args.page).await?;
            changed(ctx, &row, "page.updated").await?;
        }
        detail(ctx.conn(), args.page).await
    }

    async fn pages_delete(
        &self,
        ctx: &mut CommandCtx,
        args: PageArgs,
    ) -> Result<PageDeleted, ApiError> {
        let row = locate(ctx, args.page, Capability::PagesManage, None).await?;
        // Open streams of the page re-check access at this event and find it gone.
        ctx.record_also(&Topic::page(args.page), "page.deleted", json!({}))
            .await?;
        sqlx::query("DELETE FROM pages WHERE id = ?")
            .bind(row.id)
            .execute(ctx.conn())
            .await?;
        record_tree_ctx(
            ctx,
            Topic::pages,
            row.node(),
            "page.deleted",
            json!({ "page": args.page }),
        )
        .await?;
        Ok(PageDeleted { page: args.page })
    }

    async fn pages_create_share_token(
        &self,
        ctx: &mut CommandCtx,
        args: PagesCreateShareTokenArgs,
    ) -> Result<ShareTokenCreated, ApiError> {
        let row = locate(ctx, args.page, Capability::PagesManage, None).await?;
        let name = text("name", &args.name, true, NAME_MAX_CHARS)?;
        let now = ctx.now();
        if args.expires_at.is_some_and(|at| at <= now) {
            return Err(FieldErrors::one(
                "expires_at",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM page_share_tokens WHERE page_id = ?")
                .bind(row.id)
                .fetch_one(ctx.conn())
                .await?;
        if count as usize >= PAGE_SHARE_TOKENS_MAX {
            return Err(FieldErrors::one(
                "page",
                FieldErrorCode::TooMany,
                Some(PAGE_SHARE_TOKENS_MAX as u32),
            ));
        }
        let (secret, digest) = identity::new_session_token();
        let creator = ctx.caller().user().cloned();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO page_share_tokens (page_id, name, token_hash, expires_at, created_by,
                                            created_at)
             VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(row.id)
        .bind(&name)
        .bind(digest)
        .bind(args.expires_at)
        .bind(creator.as_ref().map(|user| user.as_str()))
        .bind(now)
        .fetch_one(ctx.conn())
        .await?;
        bump(ctx, args.page).await?;
        changed(ctx, &row, "page.updated").await?;
        let created_by = match &creator {
            Some(user) => identity::account(ctx.conn(), user)
                .await?
                .map(|account| account.name),
            None => None,
        };
        Ok(ShareTokenCreated {
            token: PageShareToken {
                id: ShareTokenId::new(id),
                name,
                expires_at: args.expires_at,
                created_by,
                created_at: now,
            },
            secret: Some(secret),
            secret_withheld: false,
        })
    }

    async fn pages_revoke_share_token(
        &self,
        ctx: &mut CommandCtx,
        args: ShareTokenArgs,
    ) -> Result<ShareTokenRevoked, ApiError> {
        let page: i64 = sqlx::query_scalar("SELECT page_id FROM page_share_tokens WHERE id = ?")
            .bind(args.token.get())
            .fetch_optional(ctx.conn())
            .await?
            .ok_or_else(ApiError::not_found)?;
        let row = locate(ctx, PageId::new(page), Capability::PagesManage, None).await?;
        // Sessions opened with the token cascade.
        sqlx::query("DELETE FROM page_share_tokens WHERE id = ?")
            .bind(args.token.get())
            .execute(ctx.conn())
            .await?;
        bump(ctx, row.page()).await?;
        changed(ctx, &row, "page.updated").await?;
        Ok(ShareTokenRevoked { token: args.token })
    }

    async fn pages_list(
        &self,
        ctx: &mut QueryCtx,
        args: PagesListArgs,
    ) -> Result<Snapshot<PageList>, ApiError> {
        ctx.node_scope(args.node, Capability::NodeRead).await?;
        #[derive(sqlx::FromRow)]
        struct Listed {
            id: i64,
            node_id: i64,
            slug: String,
            title: String,
            public: bool,
            password: bool,
            email: bool,
            token: bool,
            components: i64,
            revision: i64,
            updated_at: i64,
        }
        let rows: Vec<Listed> = sqlx::query_as(
            "WITH RECURSIVE below(id) AS (
                 SELECT ?1
                 UNION ALL
                 SELECT n.id FROM nodes n JOIN below b ON n.parent_id = b.id WHERE ?2
             )
             SELECT p.id, p.node_id, p.slug, p.title, p.public,
                    p.password_hash IS NOT NULL AS password,
                    p.allowed_emails != '[]' OR p.allowed_domains != '[]' AS email,
                    EXISTS (SELECT 1 FROM page_share_tokens t
                            WHERE t.page_id = p.id AND coalesce(t.expires_at > ?3, 1)) AS token,
                    (SELECT COUNT(*) FROM page_components pc WHERE pc.page_id = p.id)
                        AS components,
                    p.revision, p.updated_at
             FROM pages p WHERE p.node_id IN (SELECT id FROM below) ORDER BY p.title, p.id",
        )
        .bind(args.node.get())
        .bind(args.subtree)
        .bind(self.clock.now_ms())
        .fetch_all(ctx.conn())
        .await?;
        let hostnames: Vec<(i64, String)> =
            sqlx::query_as("SELECT page_id, hostname FROM page_hostnames ORDER BY hostname")
                .fetch_all(ctx.conn())
                .await?;
        let mut by_page: HashMap<i64, Vec<String>> = HashMap::new();
        for (page, hostname) in hostnames {
            by_page.entry(page).or_default().push(hostname);
        }
        Ok(Snapshot {
            topic: Topic::pages(args.node),
            data: PageList {
                pages: rows
                    .into_iter()
                    .map(|row| PageSummary {
                        access_modes: [
                            (row.password, PageAccessMode::Password),
                            (row.email, PageAccessMode::Email),
                            (row.token, PageAccessMode::Token),
                        ]
                        .into_iter()
                        .filter_map(|(set, mode)| set.then_some(mode))
                        .collect(),
                        id: PageId::new(row.id),
                        node: NodeId::new(row.node_id),
                        hostnames: by_page.remove(&row.id).unwrap_or_default(),
                        slug: row.slug,
                        title: row.title,
                        public: row.public,
                        components: row.components as u32,
                        revision: row.revision,
                        updated_at: row.updated_at,
                    })
                    .collect(),
            },
        })
    }

    async fn pages_get(
        &self,
        ctx: &mut QueryCtx,
        args: PageArgs,
    ) -> Result<Snapshot<PageDetail>, ApiError> {
        let row = load(ctx.conn(), args.page).await?;
        ctx.node_scope(row.node(), Capability::NodeRead).await?;
        Ok(Snapshot {
            topic: Topic::pages(row.node()),
            data: detail(ctx.conn(), args.page).await?,
        })
    }

    async fn pages_preview(
        &self,
        ctx: &mut QueryCtx,
        args: PagesPreviewArgs,
    ) -> Result<Snapshot<PagePreview>, ApiError> {
        let row = load(ctx.conn(), args.page).await?;
        ctx.node_scope(row.node(), Capability::NodeRead).await?;
        let locale = args
            .locale
            .filter(|locale| row.locales().contains(locale))
            .unwrap_or_else(|| row.default_locale());
        let built = snapshot::cached(self, &row, args.audience, locale).await?;
        Ok(Snapshot {
            topic: Topic::page(args.page),
            data: PagePreview {
                snapshot: (*built.snapshot).clone(),
            },
        })
    }
}
