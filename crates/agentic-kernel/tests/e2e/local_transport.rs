use agentic_kernel::test_support::e2e::{
    run_local_backend_stream, LocalBackendStreamObservation, MockLocalCompletionChunk,
};

fn observe(chunks: &[&str]) -> LocalBackendStreamObservation {
    run_local_backend_stream(
        &chunks
            .iter()
            .enumerate()
            .map(|(index, chunk)| MockLocalCompletionChunk {
                content: (*chunk).to_string(),
                stop: index + 1 == chunks.len(),
            })
            .collect::<Vec<_>>(),
    )
    .expect("run local backend stream")
}

#[test]
fn local_backend_preserves_tool_marker_for_delta_chunks() {
    let observation = observe(&["TO", r#"OL:calc {"expression":"1+1"}"#]);

    assert_eq!(
        observation.emitted_text,
        r#"TOOL:calc {"expression":"1+1"}"#
    );
    assert_eq!(
        observation.observed_chunks.concat(),
        observation.emitted_text
    );
    assert!(!observation.emitted_text.contains("TOTOOLOL"));
    assert!(!observation.finished);
}

#[test]
fn local_backend_normalizes_cumulative_chunks_into_canonical_text() {
    let observation = observe(&["TO", "TOO", r#"TOOL:calc {"expression":"1+1"}"#]);

    assert_eq!(
        observation.emitted_text,
        r#"TOOL:calc {"expression":"1+1"}"#
    );
    assert_eq!(
        observation.observed_chunks.concat(),
        observation.emitted_text
    );
    assert_eq!(
        observation.observed_chunks,
        vec!["TO", "O", r#"L:calc {"expression":"1+1"}"#]
    );
}

#[test]
fn local_backend_normalizes_overlapping_chunks_into_canonical_text() {
    let observation = observe(&["TO", r#"OOL:calc {"expression":"1+1"}"#]);

    assert_eq!(
        observation.emitted_text,
        r#"TOOL:calc {"expression":"1+1"}"#
    );
    assert_eq!(
        observation.observed_chunks.concat(),
        observation.emitted_text
    );
    assert_eq!(
        observation.observed_chunks,
        vec!["TO", r#"OL:calc {"expression":"1+1"}"#]
    );
}

#[test]
fn local_backend_preserves_single_char_overlap_when_partial_tool_marker_continues() {
    let observation = observe(&["\n\n", "TO", "OL", r#":calc {"expression":"1+1"}"#]);

    assert_eq!(
        observation.emitted_text,
        "\n\nTOOL:calc {\"expression\":\"1+1\"}"
    );
    assert_eq!(
        observation.observed_chunks,
        vec!["\n\n", "TO", "OL", r#":calc {"expression":"1+1"}"#]
    );
    assert_eq!(
        observation.observed_chunks.concat(),
        observation.emitted_text
    );
}

#[test]
fn local_backend_preserves_repeated_tool_prefix_fragments() {
    let observation = observe(&["TOOL:", "ask_human ", "TOOL:", "calc"]);

    assert_eq!(observation.emitted_text, "TOOL:ask_human TOOL:calc");
    assert_eq!(
        observation.observed_chunks.concat(),
        observation.emitted_text
    );
}
