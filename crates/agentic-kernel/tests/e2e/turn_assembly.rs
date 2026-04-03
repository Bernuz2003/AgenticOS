use agentic_control_models::AssistantSegmentKind;
use agentic_kernel::test_support::turn_assembly::{
    should_emit_turn_completion, FinalAssemblyObservation, StreamAssemblyObservation,
    TurnAssemblyHarness,
};

#[test]
fn plain_text_stream_is_captured_as_visible_output() {
    let mut harness = TurnAssemblyHarness::new();

    let final_step = harness.finish_step("ciao mondo");

    assert_eq!(
        final_step.segments,
        vec![(AssistantSegmentKind::Message, "ciao mondo".to_string())]
    );
    assert_eq!(final_step.complete_assistant_text, "ciao mondo");
    assert_eq!(
        final_step.pending_segments,
        vec![(AssistantSegmentKind::Message, "ciao mondo".to_string())]
    );
    assert_eq!(final_step.continuation_text, "ciao mondo");
    assert!(final_step.syscall_command.is_none());
}

#[test]
fn partial_tool_stream_is_withheld_until_complete() {
    let mut harness = TurnAssemblyHarness::new();

    let first = harness.push_stream(r#"TOOL:read_file {"path":"notes"#);
    assert_segments(&first, &[]);
    assert!(first.syscall_command.is_none());
    assert!(first.pending_syscall.is_none());

    let second = harness.finish_step(r#"/todo.md"}"#);
    assert_segments_final(&second, &[]);
    assert_eq!(
        second.syscall_command.as_deref(),
        Some(r#"TOOL:read_file {"path":"notes/todo.md"}"#)
    );
    assert_eq!(second.complete_assistant_text, "");
    assert_eq!(
        second.continuation_text,
        r#"TOOL:read_file {"path":"notes/todo.md"}"#
    );
    assert!(second.pending_segments.is_empty());
}

#[test]
fn text_before_tool_is_emitted_but_tool_itself_is_hidden() {
    let mut harness = TurnAssemblyHarness::new();

    let final_step =
        harness.finish_step("Analizzo il file.\nTOOL:read_file {\"path\":\"doc.txt\"}");

    assert_eq!(
        final_step.segments,
        vec![(
            AssistantSegmentKind::Message,
            "Analizzo il file.\n".to_string()
        )]
    );
    assert_eq!(final_step.complete_assistant_text, "Analizzo il file.\n");
    assert_eq!(
        final_step.syscall_command.as_deref(),
        Some(r#"TOOL:read_file {"path":"doc.txt"}"#)
    );
    assert_eq!(
        final_step.pending_segments,
        vec![(
            AssistantSegmentKind::Message,
            "Analizzo il file.\n".to_string()
        )]
    );
    assert_eq!(
        final_step.continuation_text,
        "Analizzo il file.\nTOOL:read_file {\"path\":\"doc.txt\"}"
    );
}

#[test]
fn inline_thinking_is_separated_from_visible_text() {
    let mut harness = TurnAssemblyHarness::new();

    let final_step = harness.finish_step("Prima\n<think>ragiono</think>\nDopo");

    assert_eq!(
        final_step.segments,
        vec![
            (AssistantSegmentKind::Message, "Prima\n".to_string()),
            (AssistantSegmentKind::Thinking, "ragiono".to_string()),
            (AssistantSegmentKind::Message, "\nDopo".to_string()),
        ]
    );
    assert_eq!(final_step.complete_assistant_text, "Prima\n\nDopo");
    assert_eq!(
        final_step.continuation_text,
        "Prima\n<think>ragiono</think>\nDopo"
    );
    assert_eq!(
        final_step.pending_segments,
        vec![
            (AssistantSegmentKind::Message, "Prima\n".to_string()),
            (AssistantSegmentKind::Thinking, "ragiono".to_string()),
            (AssistantSegmentKind::Message, "\nDopo".to_string()),
        ]
    );
}

#[test]
fn tool_markers_inside_thinking_do_not_dispatch() {
    let mut harness = TurnAssemblyHarness::new();

    let final_step =
        harness.finish_step("<think>TOOL:mkdir {\"path\":\"prova\"}</think>\nRisposta");

    assert!(final_step.syscall_command.is_none());
    assert_eq!(
        final_step.segments,
        vec![
            (
                AssistantSegmentKind::Thinking,
                r#"TOOL:mkdir {"path":"prova"}"#.to_string()
            ),
            (AssistantSegmentKind::Message, "\nRisposta".to_string()),
        ]
    );
    assert_eq!(final_step.complete_assistant_text, "\nRisposta");
    assert_eq!(
        final_step.continuation_text,
        "<think>TOOL:mkdir {\"path\":\"prova\"}</think>\nRisposta"
    );
}

#[test]
fn invalid_inline_mention_does_not_block_later_valid_tool_invocation() {
    let mut harness = TurnAssemblyHarness::new();

    let final_step = harness.finish_step(
        "Uso la funzione TOOL:mkdir. Ecco la chiamata:\nTOOL:mkdir {\"path\":\"prova\"}",
    );

    assert_eq!(
        final_step.segments,
        vec![(
            AssistantSegmentKind::Message,
            "Uso la funzione TOOL:mkdir. Ecco la chiamata:\n".to_string()
        )]
    );
    assert_eq!(
        final_step.syscall_command.as_deref(),
        Some(r#"TOOL:mkdir {"path":"prova"}"#)
    );
    assert_eq!(
        final_step.continuation_text,
        "Uso la funzione TOOL:mkdir. Ecco la chiamata:\nTOOL:mkdir {\"path\":\"prova\"}"
    );
}

#[test]
fn write_file_retry_transcript_extracts_the_real_invocation() {
    let mut harness = TurnAssemblyHarness::new();

    let final_step = harness.finish_step(&retry_write_file_transcript());

    assert_eq!(
        final_step.segments,
        vec![(
            AssistantSegmentKind::Message,
            concat!(
                "Il file \"snake.py\" non esiste all'interno della directory \"LLama3.1\".\n\n",
                "Devo riprovare a creare il file utilizzando il tool TOOL:write_file.\n\n"
            )
            .to_string()
        )]
    );
    assert_eq!(
        final_step.complete_assistant_text,
        concat!(
            "Il file \"snake.py\" non esiste all'interno della directory \"LLama3.1\".\n\n",
            "Devo riprovare a creare il file utilizzando il tool TOOL:write_file.\n\n"
        )
    );
    assert_eq!(
        final_step.syscall_command.as_deref(),
        Some(retry_write_file_invocation())
    );
    assert_eq!(final_step.continuation_text, retry_write_file_transcript());
}

#[test]
fn invalid_tool_like_text_stays_visible() {
    let mut harness = TurnAssemblyHarness::new();

    let final_step = harness.finish_step("TOOL:find_files per cercare i file nel workspace");

    assert_eq!(
        final_step.segments,
        vec![(
            AssistantSegmentKind::Message,
            "TOOL:find_files per cercare i file nel workspace".to_string()
        )]
    );
    assert_eq!(
        final_step.complete_assistant_text,
        "TOOL:find_files per cercare i file nel workspace"
    );
    assert!(final_step.syscall_command.is_none());
    assert_eq!(
        final_step.pending_segments,
        vec![(
            AssistantSegmentKind::Message,
            "TOOL:find_files per cercare i file nel workspace".to_string()
        )]
    );
    assert_eq!(
        final_step.continuation_text,
        "TOOL:find_files per cercare i file nel workspace"
    );
}

#[test]
fn split_tool_marker_is_buffered_across_stream_chunks() {
    let mut harness = TurnAssemblyHarness::new();

    let first = harness.push_stream("Richiesta:\n\nTO");
    assert_eq!(
        first.segments,
        vec![(AssistantSegmentKind::Message, "Richiesta:\n\n".to_string())]
    );
    assert!(first.syscall_command.is_none());
    assert!(first.pending_syscall.is_none());

    let second = harness.push_stream("OL");
    assert_segments(&second, &[]);
    assert!(second.syscall_command.is_none());
    assert!(second.pending_syscall.is_none());

    let third = harness.push_stream(r#":mkdir {"path":"prova"}"#);
    assert_segments(&third, &[]);
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
fn write_file_content_split_across_stream_chunks_is_buffered_until_complete() {
    let mut harness = TurnAssemblyHarness::new();
    let transcript = format!("Creo il file.\n{}", retry_write_file_invocation());
    let split1 = transcript
        .find("pygame\\nimport")
        .expect("find first content split")
        + "pygame\\n".len();
    let split2 = transcript
        .find("class SnakeGame")
        .expect("find second content split")
        + "class ".len();

    let first = harness.push_stream(&transcript[..split1]);
    assert_eq!(
        first.segments,
        vec![(AssistantSegmentKind::Message, "Creo il file.\n".to_string())]
    );
    assert!(first.syscall_command.is_none());
    assert!(first.pending_syscall.is_none());

    let second = harness.push_stream(&transcript[split1..split2]);
    assert_segments(&second, &[]);
    assert!(second.syscall_command.is_none());
    assert!(second.pending_syscall.is_none());

    let final_step = harness.finish_step(&transcript[split2..]);
    assert_segments_final(&final_step, &[]);
    assert_eq!(
        final_step.syscall_command.as_deref(),
        Some(retry_write_file_invocation())
    );
    assert_eq!(final_step.complete_assistant_text, "Creo il file.\n");
    assert_eq!(
        final_step.pending_segments,
        vec![(AssistantSegmentKind::Message, "Creo il file.\n".to_string())]
    );
    assert_eq!(final_step.continuation_text, transcript);
}

#[test]
fn incomplete_inline_thinking_keeps_raw_continuation_until_boundary_reset() {
    let mut harness = TurnAssemblyHarness::new();

    let first = harness.finish_step("This is a classic riddle<think>");
    assert_eq!(first.complete_assistant_text, "This is a classic riddle");
    assert_eq!(first.continuation_text, "This is a classic riddle<think>");

    let second = harness.finish_step("The answer is 0</think>\nZero remain.");
    assert_eq!(
        second.complete_assistant_text,
        "This is a classic riddle\nZero remain."
    );
    assert_eq!(
        second.continuation_text,
        "This is a classic riddle<think>The answer is 0</think>\nZero remain."
    );

    harness.reset_output_state();

    let third = harness.finish_step("Fresh answer");
    assert_eq!(third.complete_assistant_text, "Fresh answer");
    assert_eq!(third.continuation_text, "Fresh answer");
}

#[test]
fn queued_tool_dispatch_suppresses_turn_completed_events() {
    assert!(!should_emit_turn_completion(Some("WaitingForInput"), true));
    assert!(should_emit_turn_completion(
        Some("WaitingForHumanInput"),
        false,
    ));
}

fn assert_segments(
    observation: &StreamAssemblyObservation,
    expected: &[(AssistantSegmentKind, &str)],
) {
    let expected: Vec<_> = expected
        .iter()
        .map(|(kind, text)| (kind.clone(), (*text).to_string()))
        .collect();
    assert_eq!(observation.segments, expected);
}

fn assert_segments_final(
    observation: &FinalAssemblyObservation,
    expected: &[(AssistantSegmentKind, &str)],
) {
    let expected: Vec<_> = expected
        .iter()
        .map(|(kind, text)| (kind.clone(), (*text).to_string()))
        .collect();
    assert_eq!(observation.segments, expected);
}

fn retry_write_file_transcript() -> String {
    concat!(
        "Il file \"snake.py\" non esiste all'interno della directory \"LLama3.1\".\n\n",
        "Devo riprovare a creare il file utilizzando il tool TOOL:write_file.\n\n",
        r#"TOOL:write_file {"content":"import pygame\nimport sys\nimport random\n\nclass SnakeGame:\n def init(self, width=800, height=600):\n  self.width = width\n  self.height = height\n  self.score = 0\n\n def draw(self):\n  text = self.font.render(f'Score: {self.score}', True, (255, 255, 255))\n\nif name == 'main':\n game = SnakeGame()\n game.run()","path":"LLama3.1/snake.py"}"#,
        "\n\nOra, attendo la conferma dal kernel."
    )
    .to_string()
}

fn retry_write_file_invocation() -> &'static str {
    r#"TOOL:write_file {"content":"import pygame\nimport sys\nimport random\n\nclass SnakeGame:\n def init(self, width=800, height=600):\n  self.width = width\n  self.height = height\n  self.score = 0\n\n def draw(self):\n  text = self.font.render(f'Score: {self.score}', True, (255, 255, 255))\n\nif name == 'main':\n game = SnakeGame()\n game.run()","path":"LLama3.1/snake.py"}"#
}
