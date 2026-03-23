mod distribute;
mod install;
mod inventory;
mod package;
mod registry;
mod scaffold;
mod stage;
mod test;
mod validate;

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
pub use test::handle_test;
