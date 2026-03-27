use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct TimelineSnapshot {
    pub session_id: String,
    pub pid: u64,
    pub running: bool,
    pub workload: String,
    pub source: String,
    pub fallback_notice: Option<String>,
    pub error: Option<String>,
    pub items: Vec<TimelineItem>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum TimelineItemKind {
    UserMessage,
    Thinking,
    ToolCall,
    ActionCall,
    AssistantMessage,
    SystemEvent,
}

#[derive(Debug, Serialize, Clone)]
pub struct TimelineItem {
    pub id: String,
    pub kind: TimelineItemKind,
    pub text: String,
    pub status: String,
}
