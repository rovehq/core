mod catalog;
mod distribute;
mod install;
mod inventory;
mod package;
mod registry;
mod scaffold;
mod stage;
mod test;
mod validate;

pub(crate) use catalog::{
    get_catalog_entry, install_with_catalog_defaults, list_catalog, list_updates,
    public_kind_from_plugin_type, trust_badge_from_manifest_tier, upgrade_with_catalog_defaults,
};
pub(crate) use distribute::publish_source_to_registry;
pub use distribute::{handle_pack, handle_publish};
pub use install::{handle_install, handle_upgrade};
pub(crate) use install::{install_checked, upgrade_checked};
pub(crate) use inventory::resolve_installed_plugin;
pub use inventory::{
    handle_inspect, handle_inspect_filtered, handle_list, handle_list_filtered, handle_remove,
    handle_remove_filtered, handle_set_enabled, handle_set_enabled_filtered,
};
pub use scaffold::handle_new;
pub(crate) use test::call_native_tool;
pub use test::handle_test;
