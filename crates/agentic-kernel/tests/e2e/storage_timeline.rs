use agentic_kernel::test_support::storage_timeline::{
    import_legacy_timeline_once, normalize_legacy_assistant_messages_on_reopen,
    persist_single_turn_timeline, LegacyImportObservation, ReplayMessageObservation,
};

#[test]
fn session_turns_persist_user_input_and_assistant_segments() {
    let observation = persist_single_turn_timeline().expect("persist single turn timeline");

    assert_eq!(observation.turn_count, 1);
    assert_eq!(observation.message_count, 3);
    assert_eq!(observation.thinking_count, 1);
}

#[test]
fn legacy_timeline_import_is_idempotent() {
    let (first, second) = import_legacy_timeline_once().expect("import legacy timeline once");

    assert_eq!(
        first,
        LegacyImportObservation {
            imported_sessions: 1,
            imported_turns: 1,
            imported_messages: 5,
        }
    );
    assert_eq!(
        second,
        LegacyImportObservation {
            imported_sessions: 0,
            imported_turns: 0,
            imported_messages: 0,
        }
    );
}

#[test]
fn reopening_storage_normalizes_legacy_inline_thinking_and_preserves_replay_order() {
    let replay_messages = normalize_legacy_assistant_messages_on_reopen()
        .expect("normalize legacy assistant messages on reopen");

    assert_eq!(
        replay_messages,
        vec![
            ReplayMessageObservation {
                role: "user".to_string(),
                kind: "prompt".to_string(),
                content: "first prompt".to_string(),
            },
            ReplayMessageObservation {
                role: "assistant".to_string(),
                kind: "message".to_string(),
                content: "Prelude\n".to_string(),
            },
            ReplayMessageObservation {
                role: "assistant".to_string(),
                kind: "thinking".to_string(),
                content: "legacy reasoning".to_string(),
            },
            ReplayMessageObservation {
                role: "assistant".to_string(),
                kind: "message".to_string(),
                content: "\nAfter".to_string(),
            },
            ReplayMessageObservation {
                role: "user".to_string(),
                kind: "input".to_string(),
                content: "second prompt".to_string(),
            },
            ReplayMessageObservation {
                role: "assistant".to_string(),
                kind: "message".to_string(),
                content: "second answer".to_string(),
            },
        ]
    );
}
