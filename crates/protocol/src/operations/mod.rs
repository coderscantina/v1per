//! Every admin operation, grouped by domain. A new domain adds a module and one line below.

pub mod account;
pub mod api_keys;
pub mod auth;
pub mod channels;
pub mod components;
pub mod imports;
pub mod incidents;
pub mod instance;
pub mod invitations;
pub mod maintenance;
pub mod members;
pub mod monitors;
pub mod nodes;
pub mod pages;
pub mod probes;
pub mod two_factor;

crate::registry::domains!(
    account::AccountHandlers,
    api_keys::ApiKeysHandlers,
    auth::AuthHandlers,
    channels::ChannelsHandlers,
    components::ComponentsHandlers,
    imports::ImportsHandlers,
    incidents::IncidentsHandlers,
    instance::InstanceHandlers,
    members::MembersHandlers,
    monitors::MonitorsHandlers,
    invitations::InvitationsHandlers,
    maintenance::MaintenanceHandlers,
    nodes::NodesHandlers,
    pages::PagesHandlers,
    probes::ProbesHandlers,
    two_factor::TwoFactorHandlers,
);
