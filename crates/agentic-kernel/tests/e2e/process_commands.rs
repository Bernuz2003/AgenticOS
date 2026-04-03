use agentic_kernel::test_support::process_commands::{
    request_stop_output_while_running_observation, send_input_by_session_id_resume_observation,
    stop_output_flush_observation,
};

#[test]
fn send_input_by_session_id_implicitly_resumes_historical_session() {
    let observation = send_input_by_session_id_resume_observation().expect("resume observation");

    assert!(observation.response_ok);
    assert!(observation.prompt_text.contains("Prima domanda"));
    assert!(observation.prompt_text.contains("Prima risposta"));
    assert!(observation.prompt_text.contains("Seconda domanda"));
    assert!(observation
        .replay_messages
        .iter()
        .any(|(role, _, content)| role == "user" && content == "Seconda domanda"));
}

#[test]
fn stop_output_flushes_pending_assistant_segments_before_finishing_turn() {
    let observation = stop_output_flush_observation().expect("stop output observation");

    assert!(observation.response_ok);
    assert!(observation.active_turn_cleared);
    assert!(observation.pending_segments_cleared);
    assert!(observation
        .replay_messages
        .iter()
        .any(|(role, kind, content)| {
            role == "assistant" && kind == "message" && content == "Risposta parziale"
        }));
}

#[test]
fn stop_output_can_request_a_soft_stop_while_generation_is_in_flight() {
    let observation =
        request_stop_output_while_running_observation().expect("soft stop observation");

    assert!(observation.response_ok);
    assert!(observation.active_turn_preserved);
    assert!(observation.stop_requested);
}
