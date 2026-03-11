use std::collections::HashMap;

#[derive(Debug, Default)]
pub(super) struct RenderCache {
    pub(super) list_json: Option<String>,
    pub(super) info_json: HashMap<String, String>,
}
