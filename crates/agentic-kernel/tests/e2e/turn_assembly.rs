use agentic_kernel::test_support::turn_assembly::{
    should_emit_turn_completion, TurnAssemblyHarness,
};

#[test]
fn plain_text_stream_is_captured_as_visible_output() {
    let mut harness = TurnAssemblyHarness::new();

    let final_step = harness.finish_step("ciao mondo");

    assert_eq!(final_step.visible_text, "ciao mondo");
    assert_eq!(final_step.complete_assistant_text, "ciao mondo");
    assert_eq!(
        final_step.pending_assistant_segment.as_deref(),
        Some("ciao mondo")
    );
    assert!(final_step.syscall_command.is_none());
}

#[test]
fn partial_tool_stream_is_withheld_until_complete() {
    let mut harness = TurnAssemblyHarness::new();

    let first = harness.push_stream(r#"TOOL:read_file {"path":"notes"#);
    assert_eq!(first.visible_text, "");
    assert!(first.syscall_command.is_none());
    assert!(first.pending_syscall.is_none());

    let second = harness.finish_step(r#"/todo.md"}"#);
    assert_eq!(second.visible_text, "");
    assert_eq!(
        second.syscall_command.as_deref(),
        Some(r#"TOOL:read_file {"path":"notes/todo.md"}"#)
    );
    assert_eq!(second.complete_assistant_text, "");
    assert!(second.pending_assistant_segment.is_none());
}

#[test]
fn text_before_tool_is_emitted_but_tool_itself_is_hidden() {
    let mut harness = TurnAssemblyHarness::new();

    let final_step =
        harness.finish_step("Analizzo il file.\nTOOL:read_file {\"path\":\"doc.txt\"}");

    assert_eq!(final_step.visible_text, "Analizzo il file.\n");
    assert_eq!(final_step.complete_assistant_text, "Analizzo il file.\n");
    assert_eq!(
        final_step.syscall_command.as_deref(),
        Some(r#"TOOL:read_file {"path":"doc.txt"}"#)
    );
    assert_eq!(
        final_step.pending_assistant_segment.as_deref(),
        Some("Analizzo il file.\n")
    );
}

#[test]
fn invalid_inline_mention_does_not_block_later_valid_tool_invocation() {
    let mut harness = TurnAssemblyHarness::new();

    let final_step = harness.finish_step(
        "Uso la funzione TOOL:mkdir. Ecco la chiamata:\nTOOL:mkdir {\"path\":\"prova\"}",
    );

    assert_eq!(
        final_step.visible_text,
        "Uso la funzione TOOL:mkdir. Ecco la chiamata:\n"
    );
    assert_eq!(
        final_step.syscall_command.as_deref(),
        Some(r#"TOOL:mkdir {"path":"prova"}"#)
    );
}

#[test]
fn invalid_tool_like_text_stays_visible() {
    let mut harness = TurnAssemblyHarness::new();

    let final_step = harness.finish_step("TOOL:find_files per cercare i file nel workspace");

    assert_eq!(
        final_step.visible_text,
        "TOOL:find_files per cercare i file nel workspace"
    );
    assert_eq!(
        final_step.complete_assistant_text,
        "TOOL:find_files per cercare i file nel workspace"
    );
    assert!(final_step.syscall_command.is_none());
    assert_eq!(
        final_step.pending_assistant_segment.as_deref(),
        Some("TOOL:find_files per cercare i file nel workspace")
    );
}

#[test]
fn split_tool_marker_is_buffered_across_stream_chunks() {
    let mut harness = TurnAssemblyHarness::new();

    let first = harness.push_stream("Richiesta:\n\nTO");
    assert_eq!(first.visible_text, "Richiesta:\n\n");
    assert!(first.syscall_command.is_none());
    assert!(first.pending_syscall.is_none());

    let second = harness.push_stream("OL");
    assert_eq!(second.visible_text, "");
    assert!(second.syscall_command.is_none());
    assert!(second.pending_syscall.is_none());

    let third = harness.push_stream(r#":mkdir {"path":"prova"}"#);
    assert_eq!(third.visible_text, "");
    assert_eq!(
        third.syscall_command.as_deref(),
        Some(r#"TOOL:mkdir {"path":"prova"}"#)
    );
    assert_eq!(
        third.pending_syscall.as_deref(),
        Some(r#"TOOL:mkdir {"path":"prova"}"#)
    );
}

#[test]
fn queued_tool_dispatch_suppresses_turn_completed_events() {
    assert!(!should_emit_turn_completion(Some("WaitingForInput"), true));
    assert!(should_emit_turn_completion(
        Some("WaitingForHumanInput"),
        false,
    ));
}
