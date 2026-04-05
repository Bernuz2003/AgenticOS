use agentic_control_models::{
    CoreDumpReplayPatch, CoreDumpReplaySegmentPatch, CoreDumpReplayToolOutputOverride,
};
use agentic_kernel::test_support::core_dump::{
    core_dump_retention_observation, replay_core_dump_observation,
};
use agentic_kernel::test_support::e2e::KernelE2eHarness;
use serde_json::json;

#[test]
fn replay_core_dump_creates_isolated_branch_with_filtered_tool_permissions() {
    let observation = replay_core_dump_observation().expect("replay core dump observation");

    assert_ne!(observation.source_session_id, observation.replay_session_id);
    assert_ne!(observation.source_pid, observation.replay_pid);
    assert!(observation.replay_title.starts_with("[Replay]"));
    assert_eq!(observation.replay_tool_mode, "stubbed_recorded_tools");
    assert!(observation
        .original_allowed_tools
        .iter()
        .any(|tool| tool == "write_file"));
    assert!(!observation
        .replay_allowed_tools
        .iter()
        .any(|tool| tool == "write_file"));
    assert!(observation
        .replay_messages
        .iter()
        .any(|(role, _, content)| { role == "user" && content == "Prima domanda" }));
    assert!(observation
        .replay_messages
        .iter()
        .any(|(role, kind, content)| {
            role == "assistant" && kind == "message" && content == "Prima risposta"
        }));
}

#[test]
fn replay_core_dump_applies_context_patches_and_replays_stubbed_tool_outputs() {
    let mut harness = KernelE2eHarness::new().expect("kernel e2e harness");
    let pid = harness
        .spawn_interactive_process("use the calculator")
        .expect("spawn interactive process");

    harness
        .send_finished_token(pid, r#"TOOL:calc {"expression":"1+1"}"#)
        .expect("send source syscall");
    assert_eq!(harness.drain_worker(), 1);
    let (_queued_pid, tool_call_id, command) = harness.queued_syscall().expect("queued syscall");
    harness
        .send_syscall_completion(pid, tool_call_id.clone(), command.clone(), "2", true, false)
        .expect("send syscall completion");
    assert_eq!(harness.drain_syscalls(), 1);

    let dump = harness
        .capture_core_dump(pid, "replay_patch_test")
        .expect("capture core dump");
    let replay = harness
        .replay_core_dump(
            &dump.dump_id,
            Some("Patched replay"),
            None,
            Some(CoreDumpReplayPatch {
                context_segments: Some(vec![
                    CoreDumpReplaySegmentPatch {
                        kind: "user_turn".to_string(),
                        text: "counterfactual prompt\n".to_string(),
                    },
                    CoreDumpReplaySegmentPatch {
                        kind: "injected_context".to_string(),
                        text: "\nOutput:\n5\n".to_string(),
                    },
                ]),
                episodic_segments: Some(vec![CoreDumpReplaySegmentPatch {
                    kind: "summary".to_string(),
                    text: "remembered branch state".to_string(),
                }]),
                tool_output_overrides: vec![CoreDumpReplayToolOutputOverride {
                    tool_call_id: tool_call_id.clone(),
                    output: Some(json!({ "output": "5" })),
                    output_text: Some("5".to_string()),
                    warnings: None,
                    error_kind: None,
                    error_text: None,
                    effects: None,
                    duration_ms: None,
                    kill: None,
                    success: Some(true),
                }],
            }),
        )
        .expect("replay core dump");

    assert_eq!(replay.tool_mode, "stubbed_recorded_tools");
    assert_eq!(replay.initial_state, "Ready");
    assert_eq!(replay.patched_context_segments, 2);
    assert_eq!(replay.patched_episodic_segments, 1);
    assert_eq!(replay.overridden_invocations, 1);
    assert_eq!(
        harness
            .context_segment_texts(replay.pid)
            .expect("replay context"),
        vec![
            "counterfactual prompt\n".to_string(),
            "\nOutput:\n5\n".to_string()
        ]
    );
    assert_eq!(
        harness
            .episodic_segment_texts(replay.pid)
            .expect("replay episodic context"),
        vec!["remembered branch state".to_string()]
    );

    harness
        .send_finished_token(replay.pid, r#"TOOL:calc {"expression":"1+1"}"#)
        .expect("send replay syscall");
    assert_eq!(harness.drain_worker(), 1);
    assert!(harness.queued_syscall().is_none());
    assert_eq!(harness.drain_syscalls(), 1);
    assert_eq!(
        harness.process_state_label(replay.pid).as_deref(),
        Some("Ready")
    );
    assert!(harness
        .prompt_text(replay.pid)
        .expect("replay prompt")
        .contains("Output:\n5"));

    let replay_invocations = harness
        .recent_tool_invocations(replay.pid)
        .expect("replay invocations");
    assert!(replay_invocations.iter().any(|entry| {
        entry["output_text"].as_str() == Some("5") && entry["status"].as_str() == Some("completed")
    }));
}

#[test]
fn replay_core_dump_persists_branch_baseline_metadata() {
    let mut harness = KernelE2eHarness::new().expect("kernel e2e harness");
    let pid = harness
        .spawn_interactive_process("inspect the branch metadata")
        .expect("spawn interactive process");

    harness
        .send_finished_token(pid, r#"TOOL:calc {"expression":"2+2"}"#)
        .expect("send source syscall");
    assert_eq!(harness.drain_worker(), 1);
    let (_queued_pid, tool_call_id, command) = harness.queued_syscall().expect("queued syscall");
    harness
        .send_syscall_completion(pid, tool_call_id, command, "4", true, false)
        .expect("send syscall completion");
    assert_eq!(harness.drain_syscalls(), 1);

    let dump = harness
        .capture_core_dump(pid, "persist_replay_metadata")
        .expect("capture core dump");
    let replay = harness
        .replay_core_dump(&dump.dump_id, Some("Metadata replay"), None, None)
        .expect("replay core dump");

    let record = harness
        .replay_branch_record(&replay.session_id)
        .expect("load replay branch record")
        .expect("persisted replay branch record");
    assert_eq!(
        record["source_dump_id"].as_str(),
        Some(dump.dump_id.as_str())
    );
    assert_eq!(record["tool_mode"].as_str(), Some("stubbed_recorded_tools"));
    assert_eq!(
        record["replay_mode"].as_str(),
        Some("isolated_counterfactual_branch")
    );
    assert_eq!(record["stubbed_invocations"].as_u64(), Some(1));
    assert_eq!(
        record["baseline"]["replay_messages"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        record["baseline"]["tool_invocations"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        record["baseline"]["tool_invocations"][0]["command_text"].as_str(),
        Some(r#"TOOL:calc {"expression":"2+2"}"#)
    );
}

#[test]
fn retention_prunes_overflow_and_stale_core_dump_index_entries() {
    let observation = core_dump_retention_observation().expect("retention observation");

    assert_eq!(observation.remaining_dump_ids, vec!["dump-new".to_string()]);
    assert!(observation
        .pruned_dump_ids
        .iter()
        .any(|dump_id| dump_id == "dump-old"));
    assert!(observation
        .pruned_dump_ids
        .iter()
        .any(|dump_id| dump_id == "dump-missing"));
    assert_eq!(observation.stale_index_entries, 1);
}
