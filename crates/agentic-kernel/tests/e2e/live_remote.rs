use agentic_kernel::test_support::e2e::run_live_remote_completion;

#[test]
#[ignore = "requires remote backend credentials/config plus AGENTIC_E2E_REMOTE_BACKEND and AGENTIC_E2E_REMOTE_MODEL"]
fn live_remote_stream_preserves_split_tool_marker() {
    let backend_id = std::env::var("AGENTIC_E2E_REMOTE_BACKEND")
        .expect("AGENTIC_E2E_REMOTE_BACKEND must be set for this ignored test");
    let model_reference = std::env::var("AGENTIC_E2E_REMOTE_MODEL")
        .expect("AGENTIC_E2E_REMOTE_MODEL must be set for this ignored test");

    let observation = run_live_remote_completion(
        &backend_id,
        &model_reference,
        r#"Emit exactly one tool invocation in canonical form: TOOL:find_files {"pattern":"*.md"}"#,
    )
    .expect("run live remote completion");

    assert!(observation.emitted_text.contains("TOOL:"));
    assert!(!observation.observed_chunks.concat().is_empty());
    assert!(observation.observed_chunks.concat().contains("TOOL:"));
}
