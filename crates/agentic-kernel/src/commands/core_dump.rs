use agentic_control_models::{
    CoreDumpCaptureResult, CoreDumpInfoRequest, CoreDumpListRequest, CoreDumpReplayRequest,
    CoreDumpRequest,
};
use agentic_protocol::ControlErrorCode;

use crate::config::kernel_config;
use crate::core_dump::{
    capture_core_dump, core_dump_created_event, list_core_dumps, load_core_dump_info,
    replay_core_dump, CaptureCoreDumpArgs,
};
use crate::protocol;

use super::context::{CoreDumpCommandContext, ProcessCommandContext};

pub(crate) fn handle_core_dump(ctx: CoreDumpCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let CoreDumpCommandContext {
        client,
        request_id,
        runtime_registry,
        scheduler,
        session_registry,
        storage,
        turn_assembly,
        memory,
        in_flight,
        pending_events,
    } = ctx;

    if !kernel_config().core_dump.enabled {
        return protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::CoreDumpFailed,
            protocol::schema::ERROR,
            "core dump capture is disabled by configuration",
        );
    }

    let request = match parse_json::<CoreDumpRequest>(payload) {
        Ok(request) => request,
        Err(message) => {
            return protocol::response_protocol_err_typed(
                client,
                request_id,
                ControlErrorCode::CoreDumpInvalid,
                protocol::schema::ERROR,
                &message,
            );
        }
    };

    match capture_core_dump(
        CaptureCoreDumpArgs {
            runtime_registry,
            scheduler,
            session_registry,
            storage,
            turn_assembly,
            memory,
            in_flight,
        },
        request,
    ) {
        Ok(dump) => {
            if let Some(event) = core_dump_created_event(&dump) {
                pending_events.push(event);
            }
            protocol::response_protocol_ok(
                client,
                request_id,
                "COREDUMP",
                protocol::schema::COREDUMP,
                &CoreDumpCaptureResult { dump },
                None,
            )
        }
        Err(message) => protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::CoreDumpFailed,
            protocol::schema::ERROR,
            &message,
        ),
    }
}

pub(crate) fn handle_core_dump_info(ctx: CoreDumpCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let CoreDumpCommandContext {
        client,
        request_id,
        storage,
        ..
    } = ctx;

    let request = match parse_json::<CoreDumpInfoRequest>(payload) {
        Ok(request) => request,
        Err(message) => {
            return protocol::response_protocol_err_typed(
                client,
                request_id,
                ControlErrorCode::CoreDumpInfoInvalid,
                protocol::schema::ERROR,
                &message,
            );
        }
    };

    match load_core_dump_info(storage, &request.dump_id) {
        Ok(Some(response)) => protocol::response_protocol_ok(
            client,
            request_id,
            "COREDUMP_INFO",
            protocol::schema::COREDUMP_INFO,
            &response,
            None,
        ),
        Ok(None) => protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::CoreDumpNotFound,
            protocol::schema::ERROR,
            &format!("core dump '{}' not found", request.dump_id),
        ),
        Err(message) => protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::CoreDumpFailed,
            protocol::schema::ERROR,
            &message,
        ),
    }
}

pub(crate) fn handle_list_core_dumps(ctx: CoreDumpCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let CoreDumpCommandContext {
        client,
        request_id,
        storage,
        ..
    } = ctx;

    let request = if payload.is_empty() {
        CoreDumpListRequest { limit: None }
    } else {
        match parse_json::<CoreDumpListRequest>(payload) {
            Ok(request) => request,
            Err(message) => {
                return protocol::response_protocol_err_typed(
                    client,
                    request_id,
                    ControlErrorCode::ListCoreDumpsInvalid,
                    protocol::schema::ERROR,
                    &message,
                );
            }
        }
    };

    match list_core_dumps(storage, request.limit) {
        Ok(response) => protocol::response_protocol_ok(
            client,
            request_id,
            "LIST_COREDUMPS",
            protocol::schema::LIST_COREDUMPS,
            &response,
            None,
        ),
        Err(message) => protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::ListCoreDumpsInvalid,
            protocol::schema::ERROR,
            &message,
        ),
    }
}

pub(crate) fn handle_replay_core_dump(
    mut ctx: ProcessCommandContext<'_>,
    payload: &[u8],
) -> Vec<u8> {
    if !kernel_config().core_dump.enabled {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::CoreDumpReplayFailed,
            protocol::schema::ERROR,
            "core dump replay is disabled by configuration",
        );
    }

    let request = match parse_json::<CoreDumpReplayRequest>(payload) {
        Ok(request) => request,
        Err(message) => {
            return protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::CoreDumpReplayInvalid,
                protocol::schema::ERROR,
                &message,
            );
        }
    };

    match replay_core_dump(&mut ctx, request) {
        Ok(result) => protocol::response_protocol_ok(
            ctx.client,
            ctx.request_id,
            "REPLAY_COREDUMP",
            protocol::schema::REPLAY_COREDUMP,
            &result,
            None,
        ),
        Err((code, message)) => protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            code,
            protocol::schema::ERROR,
            &message,
        ),
    }
}

fn parse_json<T: serde::de::DeserializeOwned>(payload: &[u8]) -> Result<T, String> {
    serde_json::from_slice::<T>(payload).map_err(|err| err.to_string())
}
