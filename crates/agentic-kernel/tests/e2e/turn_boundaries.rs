use agentic_kernel::test_support::e2e::{HarnessProcessState, KernelE2eHarness};

#[test]
fn awaiting_turn_decision_boundary_stays_resumable_after_event_flush() {
    let mut harness = KernelE2eHarness::new().expect("kernel e2e harness");
    let pid = harness
        .spawn_interactive_process("continua se serve")
        .expect("spawn interactive process");

    harness
        .send_token_result_with_state(
            pid,
            "Risposta parziale",
            "",
            6,
            true,
            None,
            HarnessProcessState::AwaitingTurnDecision,
        )
        .expect("send awaiting-turn-decision token");
    assert_eq!(harness.drain_worker(), 1);
    assert_eq!(
        harness.process_state_label(pid).as_deref(),
        Some("AwaitingTurnDecision")
    );

    let turn_id = harness.active_turn_id(pid).expect("active turn id");

    harness.flush_events();

    assert_eq!(harness.active_turn_id(pid), Some(turn_id));
    let record = harness.turn_record(turn_id).expect("turn record");
    assert_eq!(record.status, "awaiting_turn_decision");
    assert!(record.finish_reason.is_none());
    assert!(record.completed_at_ms.is_none());

    let session_id = harness.session_id_for_pid(pid).expect("session id");
    let replay_messages = harness
        .replay_messages(&session_id)
        .expect("load replay messages");
    assert!(replay_messages.iter().any(|(role, kind, content)| {
        role == "assistant" && kind == "message" && content == "Risposta parziale"
    }));

    let prompt_text = harness.prompt_text(pid).expect("prompt text");
    let inference_prompt = harness
        .inference_prompt_text(pid)
        .expect("inference prompt text");
    assert_eq!(inference_prompt, prompt_text);

    harness
        .continue_current_turn(pid)
        .expect("continue current turn");

    assert_eq!(harness.active_turn_id(pid), Some(turn_id));
    let record = harness
        .turn_record(turn_id)
        .expect("turn record after continue");
    assert_eq!(record.status, "running");
    assert!(record.finish_reason.is_none());
    assert!(record.completed_at_ms.is_none());
    assert_eq!(harness.process_state_label(pid).as_deref(), Some("Ready"));
}

#[test]
fn soft_stop_requested_while_in_flight_finishes_the_turn_at_the_next_safe_boundary() {
    let mut harness = KernelE2eHarness::new().expect("kernel e2e harness");
    let pid = harness
        .spawn_interactive_process("fermati appena puoi")
        .expect("spawn interactive process");

    harness
        .send_stream_chunk(pid, "Risposta", true)
        .expect("send stream chunk");
    assert_eq!(harness.drain_worker(), 1);

    let turn_id = harness.active_turn_id(pid).expect("active turn id");
    harness.request_output_stop(pid);
    harness
        .send_token_result_with_state(
            pid,
            " parziale",
            "",
            4,
            false,
            None,
            HarnessProcessState::Running,
        )
        .expect("send token after stop request");
    assert_eq!(harness.drain_worker(), 1);
    assert_eq!(
        harness.process_state_label(pid).as_deref(),
        Some("WaitingForInput")
    );

    harness.flush_events();

    assert_eq!(harness.active_turn_id(pid), None);
    let record = harness.turn_record(turn_id).expect("turn record");
    assert_eq!(record.status, "completed");
    assert_eq!(record.finish_reason.as_deref(), Some("output_stopped"));
    assert!(record.completed_at_ms.is_some());

    let session_id = harness.session_id_for_pid(pid).expect("session id");
    let replay_messages = harness
        .replay_messages(&session_id)
        .expect("load replay messages");
    assert!(replay_messages.iter().any(|(role, kind, content)| {
        role == "assistant" && kind == "message" && content == "Risposta parziale"
    }));
}
