use agentic_protocol::ControlErrorCode;

use crate::protocol;
use crate::services::status_snapshot::{
    build_global_status, build_orchestration_status, build_pid_status, StatusSnapshotDeps,
};

use super::context::StatusCommandContext;

pub(crate) fn handle_status(ctx: StatusCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let StatusCommandContext {
        client,
        request_id,
        snapshot,
    } = ctx;

    let requested = String::from_utf8_lossy(payload).trim().to_string();

    if let Some(orch_id_str) = requested.strip_prefix("orch:") {
        return match orch_id_str.parse::<u64>() {
            Ok(orch_id) => respond_orchestration_status(client, request_id, &snapshot, orch_id),
            Err(_) => protocol::response_protocol_err_typed(
                client,
                request_id,
                ControlErrorCode::StatusInvalid,
                protocol::schema::ERROR,
                "Orchestration ID must be numeric (orch:<N>)",
            ),
        };
    }

    if !requested.is_empty() {
        return match requested.parse::<u64>() {
            Ok(pid) => respond_pid_status(client, request_id, &snapshot, pid),
            Err(_) => protocol::response_protocol_err_typed(
                client,
                request_id,
                ControlErrorCode::StatusInvalid,
                protocol::schema::ERROR,
                "STATUS payload must be empty, numeric PID, or legacy orch:<N>",
            ),
        };
    }

    let resp = build_global_status(&snapshot);
    let json = serde_json::to_string(&resp).expect("StatusResponse is always serializable");
    protocol::response_protocol_ok(
        client,
        request_id,
        "STATUS",
        protocol::schema::STATUS,
        &resp,
        Some(&json),
    )
}

fn respond_pid_status(
    client: &mut crate::transport::Client,
    request_id: &str,
    snapshot: &StatusSnapshotDeps<'_>,
    pid: u64,
) -> Vec<u8> {
    let Some(resp) = build_pid_status(snapshot, pid) else {
        return protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::PidNotFound,
            protocol::schema::ERROR,
            &format!("PID {} not found", pid),
        );
    };

    let json = serde_json::to_string(&resp).expect("PidStatusResponse is always serializable");
    protocol::response_protocol_ok(
        client,
        request_id,
        "STATUS",
        protocol::schema::PID_STATUS,
        &resp,
        Some(&json),
    )
}

fn respond_orchestration_status(
    client: &mut crate::transport::Client,
    request_id: &str,
    snapshot: &StatusSnapshotDeps<'_>,
    orch_id: u64,
) -> Vec<u8> {
    let Some(resp) = build_orchestration_status(snapshot, orch_id) else {
        return protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::OrchNotFound,
            protocol::schema::ERROR,
            &format!("Orchestration {} not found", orch_id),
        );
    };

    let json = serde_json::to_string(&resp).expect("OrchStatusResponse is always serializable");
    protocol::response_protocol_ok(
        client,
        request_id,
        "STATUS",
        protocol::schema::ORCHESTRATION_STATUS,
        &resp,
        Some(&json),
    )
}
