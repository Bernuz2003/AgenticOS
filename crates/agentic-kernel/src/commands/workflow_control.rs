use agentic_control_models::{ArtifactListRequest, OrchestrationStatusRequest};
use agentic_protocol::ControlErrorCode;

use crate::protocol;
use crate::services::status_snapshot::{
    build_artifact_list, build_orchestration_list, build_orchestration_status,
    build_scheduled_job_list,
};

use super::context::StatusCommandContext;

pub(crate) fn handle_list_orchestrations(ctx: StatusCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let StatusCommandContext {
        client,
        request_id,
        snapshot,
    } = ctx;

    if !String::from_utf8_lossy(payload).trim().is_empty() {
        return protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::ListOrchestrationsInvalid,
            protocol::schema::ERROR,
            "LIST_ORCHESTRATIONS does not accept a payload",
        );
    }

    respond_ok(
        client,
        request_id,
        "LIST_ORCHESTRATIONS",
        protocol::schema::LIST_ORCHESTRATIONS,
        &build_orchestration_list(&snapshot),
    )
}

pub(crate) fn handle_orchestration_status(
    ctx: StatusCommandContext<'_>,
    payload: &[u8],
) -> Vec<u8> {
    let StatusCommandContext {
        client,
        request_id,
        snapshot,
    } = ctx;

    let request = match serde_json::from_slice::<OrchestrationStatusRequest>(payload) {
        Ok(request) => request,
        Err(err) => {
            return protocol::response_protocol_err_typed(
                client,
                request_id,
                ControlErrorCode::OrchestrationStatusInvalid,
                protocol::schema::ERROR,
                &format!("Invalid orchestration status payload JSON: {err}"),
            );
        }
    };

    let Some(response) = build_orchestration_status(&snapshot, request.orchestration_id) else {
        return protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::OrchNotFound,
            protocol::schema::ERROR,
            &format!("Orchestration {} not found", request.orchestration_id),
        );
    };

    respond_ok(
        client,
        request_id,
        "ORCHESTRATION_STATUS",
        protocol::schema::ORCHESTRATION_STATUS,
        &response,
    )
}

pub(crate) fn handle_list_jobs(ctx: StatusCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let StatusCommandContext {
        client,
        request_id,
        snapshot,
    } = ctx;

    if !String::from_utf8_lossy(payload).trim().is_empty() {
        return protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::ListJobsInvalid,
            protocol::schema::ERROR,
            "LIST_JOBS does not accept a payload",
        );
    }

    respond_ok(
        client,
        request_id,
        "LIST_JOBS",
        protocol::schema::LIST_JOBS,
        &build_scheduled_job_list(&snapshot),
    )
}

pub(crate) fn handle_list_artifacts(ctx: StatusCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let StatusCommandContext {
        client,
        request_id,
        snapshot,
    } = ctx;

    let request = match serde_json::from_slice::<ArtifactListRequest>(payload) {
        Ok(request) => request,
        Err(err) => {
            return protocol::response_protocol_err_typed(
                client,
                request_id,
                ControlErrorCode::ArtifactListInvalid,
                protocol::schema::ERROR,
                &format!("Invalid artifact list payload JSON: {err}"),
            );
        }
    };

    let Some(response) =
        build_artifact_list(&snapshot, request.orchestration_id, request.task.as_deref())
    else {
        return protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::OrchNotFound,
            protocol::schema::ERROR,
            &format!("Orchestration {} not found", request.orchestration_id),
        );
    };

    respond_ok(
        client,
        request_id,
        "LIST_ARTIFACTS",
        protocol::schema::LIST_ARTIFACTS,
        &response,
    )
}

fn respond_ok<T: serde::Serialize>(
    client: &mut crate::transport::Client,
    request_id: &str,
    code: &str,
    schema_id: &str,
    response: &T,
) -> Vec<u8> {
    protocol::response_protocol_ok(
        client,
        request_id,
        code,
        schema_id,
        response,
        Some(&serde_json::to_string(response).expect("workflow control response is serializable")),
    )
}
