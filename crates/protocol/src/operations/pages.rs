//! Status pages (decision 0004, milestone 09). A page belongs to a node and shows monitors and
//! static components from that node's subtree, in ordered groups. Reading needs `node.read`,
//! everything else `pages.manage`. The public routes are described in
//! `docs/contracts/status-page-api.md` and typed in [`crate::status_page`].
//!
//! Access: a page is public or restricted. Grant holders on its node (Viewer and up) always
//! get in, with the internal snapshot. A restricted page also admits whoever passes one of its
//! modes: the page password, a sign-in link mailed to an allowed address or domain, or a share
//! token. A restricted page without modes is a team-internal page.
//!
//! Events go to the `pages:<id>` topic of the page's node and every ancestor: `page.created`,
//! `page.updated` and `page.deleted`, each with the page's ID. Everything the public snapshot
//! shows also moves the page's own `page:<id>` topic, which the public stream follows.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{NodeId, PageComponentId, PageGroupId, PageId, ShareTokenId};
use crate::operations::incidents::Target;
use crate::operations::monitors::SecretField;
use crate::status_page::{Accent, Locale, PageAudience, PageSnapshot, PageTheme};

crate::domain! {
    pub trait PagesHandlers;
    commands {
        /// Creates a page on `node`, restricted to grant holders until `pages.updateAccess`
        /// opens it. The slug must be unused (`invalid_value` on `slug`). Needs `pages.manage`.
        "pages.create" => pages_create (Person): PagesCreateArgs => PageDetail;
        /// Replaces the page's settings. Needs `pages.manage`.
        "pages.update" => pages_update (Person): PagesUpdateArgs => PageDetail;
        /// Replaces public access, the password, and the addresses and domains that may
        /// request sign-in links. A new or cleared password signs out everyone who entered
        /// the old one. Needs `pages.manage`.
        "pages.updateAccess" => pages_update_access (Person): PagesUpdateAccessArgs => PageDetail;
        /// Replaces the groups and their components in one step. A component keeps its ID
        /// while its target stays on the page, and a group while its `id` is sent. Targets
        /// must be monitors or static components on the page's node or below, each once
        /// (`invalid_value` on `groups`). Needs `pages.manage` and the page's current
        /// `revision`.
        "pages.updateLayout" => pages_update_layout (Person): PagesUpdateLayoutArgs => PageDetail;
        /// Routes requests whose `Host` is `hostname` to the page. The hostname must be
        /// unclaimed and not this instance's own (`invalid_value` on `hostname`). Needs
        /// `pages.manage`.
        "pages.claimHostname" => pages_claim_hostname (Person): PagesHostnameArgs => PageDetail;
        /// Stops routing `hostname` to the page. Needs `pages.manage`.
        "pages.releaseHostname" => pages_release_hostname (Person): PagesHostnameArgs => PageDetail;
        /// Removes the logo. Upload one with `POST /api/pages/<id>/logo`. Needs `pages.manage`.
        "pages.removeLogo" => pages_remove_logo (Person): PageArgs => PageDetail;
        /// Deletes a page with its layout, hostnames, share tokens and publications. Needs
        /// `pages.manage`.
        "pages.delete" => pages_delete (Person): PageArgs => PageDeleted;
        /// Creates a share token. Its secret is answered once: open the page at
        /// `/s/<slug>/t/<secret>` or with `?token=<secret>`. Needs `pages.manage`.
        "pages.createShareToken" => pages_create_share_token (Person, secret): PagesCreateShareTokenArgs => ShareTokenCreated;
        /// Revokes and deletes a share token; pages opened with it close at the next check.
        /// Needs `pages.manage`.
        "pages.revokeShareToken" => pages_revoke_share_token (Person): ShareTokenArgs => ShareTokenRevoked;
    }
    queries {
        /// The pages of a node, or with `subtree` of the node and every node below it, by
        /// title. Needs `node.read`. On the node's `pages:<id>` topic.
        "pages.list" => pages_list (Person, key): PagesListArgs => PageList;
        /// One page with its settings, access, layout and share tokens. Needs `node.read`. On
        /// the page node's `pages:<id>` topic.
        "pages.get" => pages_get (Person, key): PageArgs => PageDetail;
        /// The snapshot `audience` would get, in `locale` or the page's default. Needs
        /// `node.read`. On the page's `page:<id>` topic.
        "pages.preview" => pages_preview (Person, key): PagesPreviewArgs => PagePreview;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutoIncidentMode {
    /// Auto-incidents stay internal until someone publishes them to the page.
    #[default]
    Draft,
    /// Auto-incidents still open after `grace_minutes` are published to the page.
    AutoPublish,
}

/// What `pages.create` and `pages.update` set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageSettings {
    /// 1 to 64 characters: lowercase letters, digits and dashes, not at either end. Unique
    /// on the instance.
    pub slug: String,
    /// 1 to 100 characters after trimming.
    pub title: String,
    /// Up to 2,000 characters.
    #[serde(default)]
    #[ts(as = "Option<String>", optional)]
    pub description: String,
    /// Up to 1,000 characters.
    #[serde(default)]
    #[ts(as = "Option<String>", optional)]
    pub footer: String,
    #[serde(default)]
    #[ts(as = "Option<PageTheme>", optional)]
    pub theme: PageTheme,
    #[serde(default)]
    #[ts(as = "Option<Accent>", optional)]
    pub accent: Accent,
    /// IANA zone the daily bars are counted in. Absent in a request: the instance timezone.
    /// Always present in answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timezone: Option<String>,
    #[serde(default)]
    #[ts(as = "Option<Locale>", optional)]
    pub default_locale: Locale,
    /// Enabled locales; the default is always enabled. Absent: the default only.
    #[serde(default)]
    #[ts(as = "Option<Vec<Locale>>", optional)]
    pub locales: Vec<Locale>,
    #[serde(default)]
    #[ts(as = "Option<AutoIncidentMode>", optional)]
    pub auto_incidents: AutoIncidentMode,
    /// 0 to 1,440 minutes an auto-incident stays open before `auto_publish` publishes it.
    /// One resolved earlier is not published.
    #[serde(default)]
    #[ts(as = "Option<u32>", optional)]
    pub grace_minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PagesCreateArgs {
    pub node: NodeId,
    #[serde(flatten)]
    pub settings: PageSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PagesUpdateArgs {
    pub page: PageId,
    /// The page's `revision` the edit is based on; `stale_revision` with the current one
    /// otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
    #[serde(flatten)]
    pub settings: PageSettings,
}

/// Who may read a page besides grant holders on its node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageAccess {
    /// Anyone may read the page; the modes below then only matter once it is restricted.
    pub public: bool,
    /// The page password, 1 to 256 characters, stored as an argon2 hash. Answers only say
    /// whether one is set.
    #[serde(default)]
    #[ts(as = "Option<SecretField>", optional)]
    pub password: SecretField,
    /// Addresses that may request a sign-in link, each once.
    #[serde(default)]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub emails: Vec<String>,
    /// Domains, such as `example.com`, whose addresses may request one.
    #[serde(default)]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PagesUpdateAccessArgs {
    pub page: PageId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
    #[serde(flatten)]
    pub access: PageAccess,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageGroupInput {
    /// Keeps an existing group's ID; absent for a new group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<PageGroupId>,
    /// Up to 100 characters; empty shows the group without a heading.
    #[serde(default)]
    #[ts(as = "Option<String>", optional)]
    pub name: String,
    /// Shown expanded when the page opens. Default true.
    #[serde(default = "yes")]
    #[ts(as = "Option<bool>", optional)]
    pub default_open: bool,
    pub components: Vec<PageComponentInput>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageComponentInput {
    pub target: Target,
    /// 1 to 100 characters shown instead of the monitor's or component's own name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub display_name: Option<String>,
    /// Shown to grant holders only.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub internal: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PagesUpdateLayoutArgs {
    pub page: PageId,
    /// The page's current `revision`; `stale_revision` with it otherwise.
    pub expected_revision: i64,
    pub groups: Vec<PageGroupInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PagesHostnameArgs {
    pub page: PageId,
    /// A DNS name such as `status.example.com`, without scheme or port. Stored lowercase.
    pub hostname: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageArgs {
    pub page: PageId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PagesCreateShareTokenArgs {
    pub page: PageId,
    /// 1 to 100 characters, such as who it is for.
    pub name: String,
    /// When it stops working; absent: never.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ShareTokenArgs {
    pub token: ShareTokenId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PagesListArgs {
    pub node: NodeId,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub subtree: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PagesPreviewArgs {
    pub page: PageId,
    pub audience: PageAudience,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub locale: Option<Locale>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageLogo {
    pub content_type: String,
    pub size: u32,
    /// Path the logo is served at.
    pub url: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Page {
    pub id: PageId,
    pub node: NodeId,
    #[serde(flatten)]
    pub settings: PageSettings,
    pub access: PageAccess,
    pub hostnames: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub logo: Option<PageLogo>,
    /// `/s/<slug>` on this instance.
    pub path: String,
    /// Rises with every change to settings, access, layout, hostnames or logo. Pass it as
    /// `expected_revision`.
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageGroup {
    pub id: PageGroupId,
    pub name: String,
    pub default_open: bool,
    pub components: Vec<PageComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageComponent {
    pub id: PageComponentId,
    pub target: Target,
    /// The name the page shows: `display_name`, or the monitor's or component's own.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub display_name: Option<String>,
    pub internal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageShareToken {
    pub id: ShareTokenId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expires_at: Option<i64>,
    /// The person who created it, while their account exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub created_by: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageDetail {
    pub page: Page,
    pub groups: Vec<PageGroup>,
    pub share_tokens: Vec<PageShareToken>,
    /// The instance can send sign-in links: SMTP is set up.
    pub smtp_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageDeleted {
    pub page: PageId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ShareTokenCreated {
    pub token: PageShareToken,
    /// The token, shown once. `null` in a replayed answer.
    pub secret: Option<String>,
    /// True when this answer is a replay and the secret was withheld.
    pub secret_withheld: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ShareTokenRevoked {
    pub token: ShareTokenId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageSummary {
    pub id: PageId,
    pub node: NodeId,
    pub slug: String,
    pub title: String,
    pub public: bool,
    /// The restricted access modes set up on the page, in this order: `password`, `email`,
    /// `token`. They admit visitors once the page is not public.
    pub access_modes: Vec<PageAccessMode>,
    pub components: u32,
    pub hostnames: Vec<String>,
    pub revision: i64,
    pub updated_at: i64,
}

/// A way into a restricted page besides a grant on its node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PageAccessMode {
    /// A page password is set.
    Password,
    /// Sign-in links go to allowed addresses or domains.
    Email,
    /// The page has share tokens that have not expired.
    Token,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageList {
    pub pages: Vec<PageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PagePreview {
    pub snapshot: PageSnapshot,
}
