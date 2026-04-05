use serde::{Deserialize, Serialize};

use super::models::{AgentCoreDumpManifest, CoreDumpProcessMetadata, CoreDumpToolInvocation};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReplayBranchBaseline {
    pub context_segments: Vec<ReplayBranchSegment>,
    pub episodic_segments: Vec<ReplayBranchSegment>,
    pub replay_messages: Vec<ReplayBranchMessage>,
    pub tool_invocations: Vec<ReplayBranchInvocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReplayBranchSegment {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReplayBranchMessage {
    pub role: String,
    pub kind: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReplayBranchInvocation {
    pub tool_call_id: String,
    pub tool_name: String,
    pub command_text: String,
    pub status: String,
    #[serde(default)]
    pub output_text: Option<String>,
    #[serde(default)]
    pub error_kind: Option<String>,
    pub kill: bool,
}

pub(crate) fn build_replay_branch_baseline(
    manifest: &AgentCoreDumpManifest,
    process: &CoreDumpProcessMetadata,
) -> ReplayBranchBaseline {
    ReplayBranchBaseline {
        context_segments: process
            .context_state
            .segments
            .iter()
            .map(|segment| ReplayBranchSegment {
                kind: segment.kind.label().to_string(),
                text: segment.text.clone(),
            })
            .collect(),
        episodic_segments: process
            .context_state
            .episodic_segments
            .iter()
            .map(|segment| ReplayBranchSegment {
                kind: segment.kind.label().to_string(),
                text: segment.text.clone(),
            })
            .collect(),
        replay_messages: manifest
            .replay_messages
            .iter()
            .map(|message| ReplayBranchMessage {
                role: message.role.clone(),
                kind: message.kind.clone(),
                content: message.content.clone(),
            })
            .collect(),
        tool_invocations: manifest
            .tool_invocation_history
            .iter()
            .map(map_invocation)
            .collect(),
    }
}

fn map_invocation(invocation: &CoreDumpToolInvocation) -> ReplayBranchInvocation {
    ReplayBranchInvocation {
        tool_call_id: invocation.tool_call_id.clone(),
        tool_name: invocation.tool_name.clone(),
        command_text: invocation.command_text.clone(),
        status: invocation.status.clone(),
        output_text: invocation.output_text.clone(),
        error_kind: invocation.error_kind.clone(),
        kill: invocation.kill,
    }
}
