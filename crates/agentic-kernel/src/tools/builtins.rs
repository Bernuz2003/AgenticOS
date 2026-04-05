use std::collections::HashMap;

use crate::tool_registry::{HostExecutor, ToolBackendConfig, ToolRegistryEntry};

use super::api::Tool;

pub(crate) struct HostBuiltinRegistration {
    entry: ToolRegistryEntry,
    factory: fn() -> Box<dyn Tool>,
}

impl HostBuiltinRegistration {
    pub(crate) fn new(entry: ToolRegistryEntry, factory: fn() -> Box<dyn Tool>) -> Self {
        Self { entry, factory }
    }

    fn executor(&self) -> Result<HostExecutor, String> {
        match &self.entry.backend {
            ToolBackendConfig::Host { executor } => Ok(executor.clone()),
            other => Err(format!(
                "HostBuiltinRegistration for '{}' requires a host backend, got {other:?}",
                self.entry.descriptor.name
            )),
        }
    }

    fn into_parts(self) -> (ToolRegistryEntry, fn() -> Box<dyn Tool>) {
        (self.entry, self.factory)
    }
}

pub(crate) fn host_builtin_registrations() -> Vec<HostBuiltinRegistration> {
    vec![
        crate::tools::runner::python_host_builtin_registration(),
        crate::tools::runner::write_file_host_builtin_registration(),
        crate::tools::runner::read_file_host_builtin_registration(),
        crate::tools::runner::list_files_host_builtin_registration(),
        crate::tools::runner::calc_host_builtin_registration(),
        crate::tools::command_tools::exec_command_host_builtin_registration(),
        crate::tools::system_tools::get_time_host_builtin_registration(),
        crate::tools::human_tools::ask_human_host_builtin_registration(),
        crate::tools::network_tools::http_get_json_host_builtin_registration(),
        crate::tools::network_tools::download_url_host_builtin_registration(),
        crate::tools::network_tools::web_fetch_host_builtin_registration(),
        crate::tools::network_tools::web_search_host_builtin_registration(),
        crate::tools::workspace_tools::path_info_host_builtin_registration(),
        crate::tools::workspace_tools::find_files_host_builtin_registration(),
        crate::tools::workspace_tools::search_text_host_builtin_registration(),
        crate::tools::workspace_tools::read_file_range_host_builtin_registration(),
        crate::tools::workspace_tools::mkdir_host_builtin_registration(),
        crate::tools::workspace_edit_tools::append_file_host_builtin_registration(),
        crate::tools::workspace_edit_tools::replace_in_file_host_builtin_registration(),
        crate::tools::workspace_edit_tools::list_tree_host_builtin_registration(),
        crate::tools::workspace_edit_tools::diff_files_host_builtin_registration(),
    ]
}

pub(crate) fn host_builtin_registry_entries() -> Vec<ToolRegistryEntry> {
    host_builtin_registrations()
        .into_iter()
        .map(|registration| registration.into_parts().0)
        .collect()
}

pub(crate) fn host_builtin_dispatch_table() -> HashMap<HostExecutor, Box<dyn Tool>> {
    let mut builtins = HashMap::new();
    for registration in host_builtin_registrations() {
        match registration.executor() {
            Ok(executor) => {
                let (_, factory) = registration.into_parts();
                builtins.insert(executor, factory());
            }
            Err(err) => {
                tracing::error!(%err, "skipping malformed host builtin registration");
            }
        }
    }
    builtins
}
