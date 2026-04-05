use agentic_control_models::KernelEvent;
use agentic_kernel::test_support::core_dump::{
    manual_core_dump_checked_out_observation, manual_core_dump_live_observation,
};
use agentic_kernel::test_support::e2e::KernelE2eHarness;
use serde_json::json;

#[test]
fn manual_core_dump_captures_live_process_state_and_is_queryable() {
    let observation = manual_core_dump_live_observation().expect("live core dump observation");

    assert!(observation.list_dump_ids.contains(&observation.dump_id));
    assert_eq!(
        observation.manifest["format"].as_str(),
        Some("agentic_core_dump.v1")
    );
    assert_eq!(
        observation.manifest["target"]["source"].as_str(),
        Some("live_process")
    );
    assert_eq!(
        observation.manifest["target"]["pid"].as_u64(),
        Some(observation.pid)
    );
    assert_eq!(
        observation.manifest["target"]["session_id"].as_str(),
        Some(observation.session_id.as_str())
    );
    assert!(observation.manifest["process"]["prompt_text"]
        .as_str()
        .is_some_and(|prompt| prompt.contains("Prima domanda")));
    assert!(observation.manifest["turn_assembly"]["visible_projection"]
        .as_str()
        .is_some_and(|text| text.contains("Risposta parziale")));
    assert!(observation.manifest["replay_messages"]
        .as_array()
        .is_some_and(|messages| messages.iter().any(|message| {
            message["role"].as_str() == Some("user")
                && message["content"].as_str() == Some("Prima domanda")
        })));
    assert!(observation.manifest["session_audit_events"]
        .as_array()
        .is_some_and(|events| {
            events
                .iter()
                .any(|event| event["kind"].as_str() == Some("spawned"))
        }));
    assert_eq!(
        observation.manifest["backend_state"]["available"].as_bool(),
        Some(false)
    );
}

#[test]
fn manual_core_dump_preserves_checked_out_process_snapshot() {
    let observation =
        manual_core_dump_checked_out_observation().expect("checked-out core dump observation");

    assert!(observation.list_dump_ids.contains(&observation.dump_id));
    assert_eq!(
        observation.manifest["target"]["source"].as_str(),
        Some("checked_out_process")
    );
    assert_eq!(
        observation.manifest["target"]["in_flight"].as_bool(),
        Some(true)
    );
    assert!(observation.manifest["process"]["prompt_text"]
        .as_str()
        .is_some_and(|prompt| prompt.contains("Prima domanda")));
    assert!(observation.manifest["process"]["rendered_inference_prompt"]
        .as_str()
        .is_some_and(|prompt| prompt.contains("Risposta parziale")));
    assert!(observation.manifest["limitations"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| {
            item.as_str() == Some("checked_out_snapshot_captured_before_worker_completion")
        })));
}

#[test]
fn syscall_timeout_automatically_creates_a_core_dump() {
    let mut harness = KernelE2eHarness::new().expect("e2e harness");
    let pid = harness
        .spawn_interactive_process("Prima domanda")
        .expect("spawn interactive process");
    harness
        .send_syscall_completion(
            pid,
            "call-timeout-1",
            r#"TOOL:exec_command {"program":"sh","args":["-lc","sleep 10"]}"#,
            "Execution timeout for 'exec_command': 5000ms",
            false,
            true,
        )
        .expect("send syscall completion");

    assert_eq!(harness.drain_syscalls(), 1);

    let summary = harness
        .latest_core_dump()
        .expect("load latest core dump")
        .expect("latest core dump");
    assert_eq!(summary.pid, Some(pid));
    assert_eq!(summary.reason, "syscall_timeout");

    let manifest_json = harness
        .core_dump_manifest_json(&summary.dump_id)
        .expect("load manifest json")
        .expect("manifest json");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_json).expect("parse manifest json");
    assert_eq!(manifest["capture"]["mode"].as_str(), Some("automatic"));
    assert_eq!(
        manifest["capture"]["reason"].as_str(),
        Some("syscall_timeout")
    );

    let events = harness.pending_events();
    assert!(events.iter().any(|event| matches!(
        event,
        KernelEvent::CoreDumpCreated {
            pid: event_pid,
            reason,
            ..
        } if *event_pid == pid && reason == "syscall_timeout"
    )));
}

#[test]
fn core_dump_includes_replay_grade_checkpoints_and_tool_history() {
    let mut harness = KernelE2eHarness::new().expect("e2e harness");
    let pid = harness
        .spawn_interactive_process("Scrivi un file di test")
        .expect("spawn interactive process");

    harness
        .send_finished_token(
            pid,
            r#"TOOL:write_file {"path":"notes/out.txt","content":"ciao"}"#,
        )
        .expect("send finished token");
    assert_eq!(harness.drain_worker(), 1);

    let (_queued_pid, tool_call_id, command) = harness.queued_syscall().expect("queued syscall");
    harness
        .send_syscall_completion_with_details(
            pid,
            tool_call_id.clone(),
            command.clone(),
            "Wrote 4 bytes to 'notes/out.txt'.",
            true,
            false,
            Some(json!({
                "output": "Wrote 4 bytes to 'notes/out.txt'.",
                "path": "notes/out.txt",
                "bytes_written": 4,
                "created": true,
            })),
            Vec::new(),
            None,
            vec![json!({
                "kind": "workspace_write",
                "tool_name": "write_file",
                "path": "notes/out.txt",
                "bytes_written": 4,
                "created": true,
            })],
        )
        .expect("send syscall completion");
    assert_eq!(harness.drain_syscalls(), 1);

    let checkpoints = harness
        .recent_debug_checkpoints(pid)
        .expect("recent checkpoints");
    let checkpoint_boundaries: Vec<_> = checkpoints
        .iter()
        .filter_map(|checkpoint| checkpoint["boundary"].as_str())
        .collect();
    assert!(checkpoint_boundaries.contains(&"canonical_commit"));
    assert!(checkpoint_boundaries.contains(&"syscall_dispatched"));
    assert!(checkpoint_boundaries.contains(&"syscall_completed"));

    let tool_history = harness
        .recent_tool_invocations(pid)
        .expect("recent tool invocations");
    assert_eq!(tool_history.len(), 1);
    assert_eq!(
        tool_history[0]["tool_call_id"].as_str(),
        Some(tool_call_id.as_str())
    );
    assert_eq!(tool_history[0]["tool_name"].as_str(), Some("write_file"));
    assert_eq!(tool_history[0]["status"].as_str(), Some("completed"));
    assert_eq!(
        tool_history[0]["input"]["path"].as_str(),
        Some("notes/out.txt")
    );
    assert_eq!(
        tool_history[0]["output"]["path"].as_str(),
        Some("notes/out.txt")
    );
    assert!(tool_history[0]["effects"]
        .as_array()
        .is_some_and(|effects| effects.iter().any(|effect| {
            effect["kind"].as_str() == Some("workspace_write")
                && effect["path"].as_str() == Some("notes/out.txt")
        })));

    let summary = harness
        .capture_core_dump(pid, "replay_grade_history_test")
        .expect("capture core dump");
    let manifest_json = harness
        .core_dump_manifest_json(&summary.dump_id)
        .expect("load manifest")
        .expect("manifest json");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_json).expect("parse manifest json");

    assert!(manifest["debug_checkpoints"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| {
            item["boundary"].as_str() == Some("syscall_dispatched")
                && item["snapshot"]["invocation"]["tool_call_id"].as_str()
                    == Some(tool_call_id.as_str())
        })));
    assert!(manifest["tool_invocation_history"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| {
            item["tool_call_id"].as_str() == Some(tool_call_id.as_str())
                && item["effects"].as_array().is_some_and(|effects| {
                    effects.iter().any(|effect| {
                        effect["kind"].as_str() == Some("workspace_write")
                            && effect["path"].as_str() == Some("notes/out.txt")
                    })
                })
        })));
}
