use agentic_control_models::{InvocationEvent, InvocationStatus, KernelEvent};
use agentic_kernel::test_support::e2e::{
    run_local_backend_stream, KernelE2eHarness, MockLocalCompletionChunk,
};

#[test]
fn local_tool_invocation_dispatches_without_corrupting_tool_marker() {
    let observation = run_local_backend_stream(&[
        MockLocalCompletionChunk {
            content: "TO".to_string(),
            stop: false,
        },
        MockLocalCompletionChunk {
            content: "TOO".to_string(),
            stop: false,
        },
        MockLocalCompletionChunk {
            content: r#"TOOL:calc {"expression":"1+1"}"#.to_string(),
            stop: true,
        },
    ])
    .expect("local backend observation");

    let mut harness = KernelE2eHarness::new().expect("kernel e2e harness");
    let pid = harness
        .spawn_interactive_process("use the calculator")
        .expect("spawn interactive process");

    harness
        .send_finished_token(pid, observation.emitted_text.clone())
        .expect("send finished token");
    assert_eq!(harness.drain_worker(), 1);

    assert_eq!(
        harness.process_state_label(pid).as_deref(),
        Some("WaitingForSyscall")
    );

    let queued = harness.queued_syscall().expect("queued syscall");
    assert_eq!(queued.0, pid);
    assert_eq!(queued.2, r#"TOOL:calc {"expression":"1+1"}"#);

    let invocation = pending_invocation(&harness.pending_events()).expect("invocation event");
    assert_eq!(invocation.status, InvocationStatus::Dispatched);
    assert_eq!(invocation.command, r#"TOOL:calc {"expression":"1+1"}"#);
    assert!(timeline_chunks(&harness.pending_events())
        .iter()
        .all(|text| !text.contains("TOOL:calc")));

    harness.flush_events();

    let session_id = harness.session_id_for_pid(pid).expect("session id");
    let replay_messages = harness
        .replay_messages(&session_id)
        .expect("load replay messages");
    assert!(replay_messages.iter().any(|(role, kind, content)| {
        role == "system"
            && kind == "invocation"
            && serde_json::from_str::<InvocationEvent>(content)
                .map(|event| event.command == r#"TOOL:calc {"expression":"1+1"}"#)
                .unwrap_or(false)
    }));
}

#[test]
fn remote_split_marker_dispatches_without_timeline_leak_and_persists_ordered_segments() {
    let mut harness = KernelE2eHarness::new().expect("kernel e2e harness");
    let pid = harness
        .spawn_interactive_process("stream a tool")
        .expect("spawn interactive process");

    for (index, chunk) in [
        "Analizzo i file:\n\nTO",
        "OL",
        r#":find_files {"pattern":"*.md"}"#,
    ]
    .into_iter()
    .enumerate()
    {
        harness
            .send_stream_chunk(pid, chunk, index == 0)
            .expect("send stream chunk");
        assert_eq!(harness.drain_worker(), 1);
    }

    assert_eq!(
        harness.checked_out_pending_syscall(pid).as_deref(),
        Some(r#"TOOL:find_files {"pattern":"*.md"}"#)
    );

    harness
        .send_finished_token(pid, "")
        .expect("send final token");
    assert_eq!(harness.drain_worker(), 1);

    let queued = harness.queued_syscall().expect("queued syscall");
    assert_eq!(queued.2, r#"TOOL:find_files {"pattern":"*.md"}"#);
    assert_eq!(
        harness.process_state_label(pid).as_deref(),
        Some("WaitingForSyscall")
    );

    let invocation = pending_invocation(&harness.pending_events()).expect("invocation event");
    assert_eq!(invocation.status, InvocationStatus::Dispatched);
    assert_eq!(invocation.command, r#"TOOL:find_files {"pattern":"*.md"}"#);
    assert!(timeline_chunks(&harness.pending_events())
        .iter()
        .all(|text| !text.contains("TOOL:find_files")));

    harness.flush_events();

    let session_id = harness.session_id_for_pid(pid).expect("session id");
    let replay_messages = harness
        .replay_messages(&session_id)
        .expect("load replay messages");
    assert_eq!(
        replay_messages
            .iter()
            .filter(|(role, kind, _)| role == "assistant" && kind == "message")
            .count(),
        1
    );
    assert!(replay_messages.iter().any(|(role, kind, content)| {
        role == "assistant" && kind == "message" && content.contains("Analizzo i file:")
    }));
    assert!(replay_messages.iter().any(|(role, kind, content)| {
        role == "system"
            && kind == "invocation"
            && serde_json::from_str::<InvocationEvent>(content)
                .map(|event| event.command == r#"TOOL:find_files {"pattern":"*.md"}"#)
                .unwrap_or(false)
    }));
}

#[test]
fn syscall_completion_reinjects_output_and_records_structured_completion() {
    let mut harness = KernelE2eHarness::new().expect("kernel e2e harness");
    let pid = harness
        .spawn_interactive_process("use the calculator")
        .expect("spawn interactive process");

    harness
        .send_finished_token(pid, r#"TOOL:calc {"expression":"1+1"}"#)
        .expect("send finished token");
    assert_eq!(harness.drain_worker(), 1);

    let (_queued_pid, tool_call_id, command) = harness.queued_syscall().expect("queued syscall");
    harness
        .send_syscall_completion(pid, tool_call_id.clone(), command.clone(), "2", true, false)
        .expect("send syscall completion");
    assert_eq!(harness.drain_syscalls(), 1);

    let invocation = pending_invocation(&harness.pending_events()).expect("invocation event");
    assert_eq!(invocation.status, InvocationStatus::Completed);
    assert_eq!(harness.process_state_label(pid).as_deref(), Some("Ready"));
    assert!(harness
        .prompt_text(pid)
        .expect("prompt text")
        .contains("Output:\n2"));

    harness.flush_events();

    let session_id = harness.session_id_for_pid(pid).expect("session id");
    let replay_messages = harness
        .replay_messages(&session_id)
        .expect("load replay messages");
    assert!(replay_messages.iter().any(|(role, kind, content)| {
        role == "system"
            && kind == "invocation"
            && serde_json::from_str::<InvocationEvent>(content)
                .map(|event| {
                    event.invocation_id == tool_call_id
                        && event.status == InvocationStatus::Completed
                })
                .unwrap_or(false)
    }));

    let audit_kinds = harness.recent_audit_kinds(pid).expect("audit kinds");
    assert!(audit_kinds.iter().any(|kind| kind == "completed"));
}

#[test]
fn plain_stream_text_persists_as_single_consolidated_assistant_segment() {
    let mut harness = KernelE2eHarness::new().expect("kernel e2e harness");
    let pid = harness
        .spawn_interactive_process("reply without tools")
        .expect("spawn interactive process");

    for (index, chunk) in ["Questa ", "risposta ", "arriva ", "a pezzi."]
        .into_iter()
        .enumerate()
    {
        harness
            .send_stream_chunk(pid, chunk, index == 0)
            .expect("send stream chunk");
        assert_eq!(harness.drain_worker(), 1);
    }

    harness
        .send_finished_token(pid, "")
        .expect("send final token");
    assert_eq!(harness.drain_worker(), 1);
    assert_eq!(
        harness.process_state_label(pid).as_deref(),
        Some("WaitingForInput")
    );

    harness.flush_events();

    let session_id = harness.session_id_for_pid(pid).expect("session id");
    let replay_messages = harness
        .replay_messages(&session_id)
        .expect("load replay messages");
    let assistant_messages: Vec<_> = replay_messages
        .iter()
        .filter(|(role, kind, _)| role == "assistant" && kind == "message")
        .collect();
    assert_eq!(assistant_messages.len(), 1);
    assert_eq!(assistant_messages[0].2, "Questa risposta arriva a pezzi.");
}

#[test]
fn invalid_tool_like_text_is_not_dispatched_and_remains_assistant_text() {
    let mut harness = KernelE2eHarness::new().expect("kernel e2e harness");
    let pid = harness
        .spawn_interactive_process("mention a tool in prose")
        .expect("spawn interactive process");

    harness
        .send_finished_token(pid, "TOOL:find_files per cercare i file nel workspace")
        .expect("send finished token");
    assert_eq!(harness.drain_worker(), 1);
    assert_eq!(
        harness.process_state_label(pid).as_deref(),
        Some("WaitingForInput")
    );
    assert!(harness.queued_syscall().is_none());
    assert!(pending_invocation(&harness.pending_events()).is_none());

    harness.flush_events();

    let session_id = harness.session_id_for_pid(pid).expect("session id");
    let replay_messages = harness
        .replay_messages(&session_id)
        .expect("load replay messages");
    assert!(replay_messages.iter().any(|(role, kind, content)| {
        role == "assistant"
            && kind == "message"
            && content == "TOOL:find_files per cercare i file nel workspace"
    }));
    assert!(!replay_messages
        .iter()
        .any(|(role, kind, _)| role == "system" && kind == "invocation"));
}

fn pending_invocation(events: &[KernelEvent]) -> Option<InvocationEvent> {
    events.iter().rev().find_map(|event| match event {
        KernelEvent::InvocationUpdated { invocation, .. } => Some(invocation.clone()),
        _ => None,
    })
}

fn timeline_chunks(events: &[KernelEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            KernelEvent::TimelineChunk { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}
