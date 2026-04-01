use agentic_control_models::InvocationEvent;
use agentic_kernel::test_support::e2e::{
    run_live_local_completion, run_live_local_exec_completion, KernelE2eHarness,
};

#[test]
#[ignore = "requires a configured external llama.cpp runtime"]
fn live_local_llamacpp_preserves_canonical_tool_marker() {
    let observation = run_live_local_completion(
        r#"Rispondi esattamente con TOOL:calc {"expression":"1847*23"} e nulla altro."#,
    )
    .expect("run live local completion");

    eprintln!("live emitted_text = {:?}", observation.emitted_text);
    for (index, chunk) in observation.observed_chunks.iter().enumerate() {
        eprintln!("live chunk[{index}] = {:?}", chunk);
    }

    assert!(observation.emitted_text.trim_start().starts_with("TOOL:"));
    assert!(!observation.emitted_text.contains("TOTOOLOL"));
    assert_eq!(
        observation.observed_chunks.concat(),
        observation.emitted_text
    );
}

#[test]
#[ignore = "requires a configured external llama.cpp runtime"]
fn live_local_tool_invocation_replays_cleanly_through_kernel_storage_and_audit() {
    let observation = run_live_local_completion(
        r#"Rispondi esattamente con TOOL:calc {"expression":"1847*23"} e nulla altro."#,
    )
    .expect("run live local completion");

    eprintln!("live emitted_text = {:?}", observation.emitted_text);
    for (index, chunk) in observation.observed_chunks.iter().enumerate() {
        eprintln!("live chunk[{index}] = {:?}", chunk);
    }

    let mut harness = KernelE2eHarness::new().expect("kernel e2e harness");
    let pid = harness
        .spawn_interactive_process("calcola 1+1")
        .expect("spawn interactive process");

    for (index, chunk) in observation.observed_chunks.iter().enumerate() {
        harness
            .send_stream_chunk(pid, chunk.clone(), index == 0)
            .expect("send stream chunk");
        assert_eq!(harness.drain_worker(), 1);
    }

    let streamed_text = observation.observed_chunks.concat();
    let final_suffix = observation
        .emitted_text
        .strip_prefix(&streamed_text)
        .unwrap_or_default()
        .to_string();
    harness
        .send_token_result(
            pid,
            final_suffix,
            observation.generated_tokens,
            observation.finished,
            None,
        )
        .expect("send final token");
    assert_eq!(harness.drain_worker(), 1);

    let queued = harness.queued_syscall().expect("queued syscall");
    eprintln!("queued syscall = {:?}", queued.2);
    assert_eq!(
        harness.process_state_label(pid).as_deref(),
        Some("WaitingForSyscall")
    );
    assert!(queued.2.starts_with("TOOL:calc "));
    assert!(!queued.2.contains("TOTOOLOL"));

    harness.flush_events();

    let session_id = harness.session_id_for_pid(pid).expect("session id");
    let replay_messages = harness
        .replay_messages(&session_id)
        .expect("load replay messages");
    let audit_kinds = harness.recent_audit_kinds(pid).expect("audit kinds");
    eprintln!("replay_messages = {replay_messages:#?}");
    eprintln!("audit_kinds = {audit_kinds:?}");

    assert!(replay_messages
        .iter()
        .all(|(role, _, content)| { role != "assistant" || !content.contains("TOOL:") }));
    assert!(replay_messages.iter().any(|(role, kind, content)| {
        role == "system"
            && kind == "invocation"
            && serde_json::from_str::<InvocationEvent>(content)
                .map(|event| event.command == queued.2)
                .unwrap_or(false)
    }));
    assert!(audit_kinds
        .iter()
        .any(|kind| kind == "first_chunk_received"));
    assert!(audit_kinds.iter().any(|kind| kind == "dispatched"));
}

#[test]
#[ignore = "requires a configured external llama.cpp runtime and reproduces the real EXEC prompt path"]
fn live_local_exec_rendered_prompt_preserves_canonical_tool_marker() {
    let observation = run_live_local_exec_completion(
        r#"Rispondi esattamente con TOOL:calc {"expression":"1847*23"} e nulla altro."#,
    )
    .expect("run live local exec completion");

    eprintln!("exec request_prompt = {:?}", observation.request_prompt);
    eprintln!("exec emitted_text = {:?}", observation.emitted_text);
    for (index, chunk) in observation.observed_chunks.iter().enumerate() {
        eprintln!("exec chunk[{index}] = {:?}", chunk);
    }

    assert!(observation.emitted_text.contains("TOOL:"));
    assert!(!observation.emitted_text.contains("TOTOOLOL"));
    assert_eq!(
        observation.observed_chunks.concat(),
        observation.emitted_text
    );
}

#[test]
#[ignore = "requires a configured external llama.cpp runtime and covers a natural tool request on the real EXEC prompt path"]
fn live_local_exec_rendered_prompt_handles_natural_tool_request_without_marker_corruption() {
    let observation = run_live_local_exec_completion(
        "Usa il tool calc per moltiplicare 1847*23. Rispondi solo con la tool invocation corretta.",
    )
    .expect("run live local exec completion");

    eprintln!("natural request_prompt = {:?}", observation.request_prompt);
    eprintln!("natural emitted_text = {:?}", observation.emitted_text);
    for (index, chunk) in observation.observed_chunks.iter().enumerate() {
        eprintln!("natural chunk[{index}] = {:?}", chunk);
    }

    assert!(observation.emitted_text.contains("TOOL:"));
    assert!(!observation.emitted_text.contains("TOTOOLOL"));
}
