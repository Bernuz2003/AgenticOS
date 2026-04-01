use agent_workspace_lib::test_support::live_timeline::{
    apply_kernel_event, finish_session_with_reason, TimelineItemKind, TimelineSeedMessage,
    TimelineSeedSession, TimelineSeedTurn, TimelineStore,
};
use agentic_control_models::{InvocationEvent, InvocationKind, InvocationStatus, KernelEvent};

#[test]
fn live_timeline_keeps_thinking_streaming_without_raw_tool_parsing() {
    let mut store = TimelineStore::default();
    apply_kernel_event(
        &mut store,
        &KernelEvent::SessionStarted {
            session_id: "sess-think".to_string(),
            pid: 1,
            workload: "general".to_string(),
            prompt: "prompt".to_string(),
        },
    );
    apply_kernel_event(
        &mut store,
        &KernelEvent::TimelineChunk {
            pid: 1,
            text: "Prelude\n<think>reasoning in progress".to_string(),
        },
    );

    let timeline = store.snapshot(1).expect("live snapshot");
    assert_eq!(timeline.items.len(), 3);
    assert!(matches!(
        timeline.items[1].kind,
        TimelineItemKind::AssistantMessage
    ));
    assert_eq!(timeline.items[1].text, "Prelude");
    assert_eq!(timeline.items[1].status, "complete");
    assert!(matches!(timeline.items[2].kind, TimelineItemKind::Thinking));
    assert_eq!(timeline.items[2].text, "reasoning in progress");
    assert_eq!(timeline.items[2].status, "streaming");
}

#[test]
fn live_timeline_does_not_infer_tool_blocks_from_raw_assistant_chunks() {
    let mut store = TimelineStore::default();
    apply_kernel_event(
        &mut store,
        &KernelEvent::SessionStarted {
            session_id: "sess-raw-tool".to_string(),
            pid: 7,
            workload: "general".to_string(),
            prompt: "prompt".to_string(),
        },
    );

    for text in ["TO", "OL", ":calc {\"expression\":\"2+2\"}"] {
        apply_kernel_event(
            &mut store,
            &KernelEvent::TimelineChunk {
                pid: 7,
                text: text.to_string(),
            },
        );
    }

    let timeline = store.snapshot(7).expect("live snapshot");
    assert_eq!(timeline.items.len(), 2);
    assert!(matches!(
        timeline.items[1].kind,
        TimelineItemKind::AssistantMessage
    ));
    assert_eq!(timeline.items[1].text, r#"TOOL:calc {"expression":"2+2"}"#);
    assert_eq!(timeline.items[1].status, "streaming");
    assert!(!timeline
        .items
        .iter()
        .any(|item| matches!(item.kind, TimelineItemKind::ToolCall)));
    assert!(!timeline
        .items
        .iter()
        .any(|item| item.text.contains("TOTOOLOL")));
}

#[test]
fn live_timeline_renders_tool_only_from_structured_invocation_events() {
    let mut store = TimelineStore::default();
    apply_kernel_event(
        &mut store,
        &KernelEvent::SessionStarted {
            session_id: "sess-invocation".to_string(),
            pid: 55,
            workload: "general".to_string(),
            prompt: "prompt".to_string(),
        },
    );
    apply_kernel_event(
        &mut store,
        &KernelEvent::TimelineChunk {
            pid: 55,
            text: "\n\n".to_string(),
        },
    );
    apply_kernel_event(
        &mut store,
        &KernelEvent::InvocationUpdated {
            pid: 55,
            invocation: InvocationEvent {
                invocation_id: "tool-55-1".to_string(),
                kind: InvocationKind::Tool,
                command: r#"TOOL:calc {"expression":"1847*23"}"#.to_string(),
                status: InvocationStatus::Dispatched,
            },
        },
    );

    let timeline = store.snapshot(55).expect("live snapshot");
    assert_eq!(timeline.items.len(), 2);
    assert!(matches!(timeline.items[1].kind, TimelineItemKind::ToolCall));
    assert_eq!(
        timeline.items[1].text,
        r#"TOOL:calc {"expression":"1847*23"}"#
    );
    assert_eq!(timeline.items[1].status, "dispatching");
    assert!(!timeline
        .items
        .iter()
        .any(|item| item.text.contains("TOTOOLOL")));
}

#[test]
fn started_session_resets_live_state_when_pid_is_reused() {
    let mut store = TimelineStore::default();
    store.insert_started_session(
        42,
        "sess-old".to_string(),
        "old prompt".to_string(),
        "general".to_string(),
    );
    store.append_assistant_chunk(42, "old output");
    finish_session_with_reason(&mut store, 42, Some("completed"));

    store.insert_started_session(
        42,
        "sess-new".to_string(),
        "new prompt".to_string(),
        "general".to_string(),
    );

    let timeline = store
        .snapshot_for_session_id("sess-new")
        .expect("new session timeline should exist");
    assert_eq!(timeline.items.len(), 2);
    assert!(matches!(
        timeline.items[0].kind,
        TimelineItemKind::UserMessage
    ));
    assert_eq!(timeline.items[0].text, "new prompt");
    assert!(matches!(
        timeline.items[1].kind,
        TimelineItemKind::AssistantMessage
    ));
    assert_eq!(timeline.items[1].status, "streaming");
    assert!(timeline.running);
}

#[test]
fn seeded_session_can_rebind_to_new_pid_without_losing_history() {
    let mut store = TimelineStore::default();
    store.insert_seeded_session(TimelineSeedSession {
        session_id: "sess-history".to_string(),
        pid: 21,
        workload: "general".to_string(),
        turns: vec![TimelineSeedTurn {
            prompt: "persisted prompt".to_string(),
            messages: vec![TimelineSeedMessage::Assistant(
                "persisted answer".to_string(),
            )],
            running: false,
        }],
        error: None,
        system_events: Vec::new(),
    });

    store.rebind_session_pid("sess-history", 84, "general".to_string());
    store.append_user_turn(84, "new input".to_string());

    assert!(store.snapshot(21).is_none());

    let timeline = store
        .snapshot(84)
        .expect("rebound session should be addressable by the new pid");
    assert_eq!(timeline.session_id, "sess-history");
    assert_eq!(timeline.items.len(), 4);
    assert_eq!(timeline.items[0].text, "persisted prompt");
    assert_eq!(timeline.items[1].text, "persisted answer");
    assert_eq!(timeline.items[2].text, "new input");
    assert!(matches!(
        timeline.items[3].kind,
        TimelineItemKind::AssistantMessage
    ));
    assert_eq!(timeline.items[3].status, "streaming");
    assert!(timeline.running);
}
