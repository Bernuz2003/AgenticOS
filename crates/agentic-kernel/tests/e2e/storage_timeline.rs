use agentic_kernel::test_support::storage_timeline::{
    import_legacy_timeline_once, persist_single_turn_timeline, LegacyImportObservation,
};

#[test]
fn session_turns_persist_user_input_and_assistant_segments() {
    let observation = persist_single_turn_timeline().expect("persist single turn timeline");

    assert_eq!(observation.turn_count, 1);
    assert_eq!(observation.message_count, 2);
}

#[test]
fn legacy_timeline_import_is_idempotent() {
    let (first, second) = import_legacy_timeline_once().expect("import legacy timeline once");

    assert_eq!(
        first,
        LegacyImportObservation {
            imported_sessions: 1,
            imported_turns: 1,
            imported_messages: 3,
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
