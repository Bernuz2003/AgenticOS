import { invoke } from "@tauri-apps/api/core";

import type {
  CoreDumpInfo,
  CoreDumpManifestPreview,
  CoreDumpSummary,
  CoreDumpSummaryDto,
  ReplayCoreDumpResult,
  ReplayCoreDumpResultDto,
} from "./index";

export async function listCoreDumps(limit?: number | null): Promise<CoreDumpSummary[]> {
  const response = await invoke<{ dumps: CoreDumpSummaryDto[] }>("list_core_dumps", {
    limit: limit ?? null,
  });
  return response.dumps.map(normalizeCoreDumpSummary);
}

export async function captureCoreDump(payload: {
  sessionId?: string | null;
  pid?: number | null;
  reason?: string | null;
  note?: string | null;
}): Promise<CoreDumpSummary> {
  const dump = await invoke<CoreDumpSummaryDto>("capture_core_dump", {
    sessionId: payload.sessionId ?? null,
    pid: payload.pid ?? null,
    reason: payload.reason ?? null,
    note: payload.note ?? null,
  });
  return normalizeCoreDumpSummary(dump);
}

export async function fetchCoreDumpInfo(dumpId: string): Promise<CoreDumpInfo> {
  const response = await invoke<{
    dump: CoreDumpSummaryDto;
    manifest_json: string;
  }>("fetch_core_dump_info", { dumpId });
  return {
    dump: normalizeCoreDumpSummary(response.dump),
    manifestJson: response.manifest_json,
    manifest: parseManifestPreview(response.manifest_json),
  };
}

export async function replayCoreDump(
  dumpId: string,
  branchLabel?: string | null,
): Promise<ReplayCoreDumpResult> {
  const response = await invoke<ReplayCoreDumpResultDto>("replay_core_dump", {
    dumpId,
    branchLabel: branchLabel ?? null,
  });
  return normalizeReplayCoreDumpResult(response);
}

function normalizeCoreDumpSummary(summary: CoreDumpSummaryDto): CoreDumpSummary {
  return {
    dumpId: summary.dump_id,
    createdAtMs: summary.created_at_ms,
    sessionId: summary.session_id,
    pid: summary.pid,
    reason: summary.reason,
    fidelity: summary.fidelity,
    path: summary.path,
    bytes: summary.bytes,
    sha256: summary.sha256,
    note: summary.note,
  };
}

function normalizeReplayCoreDumpResult(
  result: ReplayCoreDumpResultDto,
): ReplayCoreDumpResult {
  return {
    sourceDumpId: result.source_dump_id,
    sessionId: result.session_id,
    pid: result.pid,
    runtimeId: result.runtime_id,
    replaySessionTitle: result.replay_session_title,
    replayFidelity: result.replay_fidelity,
    replayMode: result.replay_mode,
    toolMode: result.tool_mode,
    initialState: result.initial_state,
    patchedContextSegments: result.patched_context_segments,
    patchedEpisodicSegments: result.patched_episodic_segments,
    stubbedInvocations: result.stubbed_invocations,
    overriddenInvocations: result.overridden_invocations,
  };
}

function parseManifestPreview(manifestJson: string): CoreDumpManifestPreview {
  try {
    const parsed = JSON.parse(manifestJson) as Record<string, unknown>;
    const capture = recordValue(parsed.capture);
    const target = recordValue(parsed.target);
    const session = optionalRecordValue(parsed.session);
    const process = optionalRecordValue(parsed.process);
    const workspace = optionalRecordValue(parsed.workspace);

    return {
      format: stringValue(parsed.format),
      capture: {
        mode: stringValue(capture.mode),
        reason: stringValue(capture.reason),
        fidelity: stringValue(capture.fidelity),
        note: nullableStringValue(capture.note),
      },
      target: {
        source: stringValue(target.source),
        state: stringValue(target.state),
        sessionId: nullableStringValue(target.session_id),
        pid: nullableNumberValue(target.pid),
        runtimeId: nullableStringValue(target.runtime_id),
        inFlight: nullableBooleanValue(target.in_flight),
      },
      session: session
        ? {
            title: nullableStringValue(session.title),
            state: nullableStringValue(session.state),
          }
        : null,
      process: process
        ? {
            toolCaller: nullableStringValue(process.tool_caller),
            tokenCount: nullableNumberValue(process.token_count),
            terminationReason: nullableStringValue(process.termination_reason),
            renderedPromptChars: stringValue(process.rendered_inference_prompt)?.length ?? null,
            promptChars: stringValue(process.prompt_text)?.length ?? null,
          }
        : null,
      counts: {
        replayMessages: arrayLength(parsed.replay_messages),
        debugCheckpoints: arrayLength(parsed.debug_checkpoints),
        toolInvocations: arrayLength(parsed.tool_invocation_history),
        toolAuditLines: arrayLength(parsed.tool_audit_lines),
        sessionAuditEvents: arrayLength(parsed.session_audit_events),
        workspaceEntries: workspace ? arrayLength(workspace.entries) : 0,
        limitations: arrayLength(parsed.limitations),
      },
      limitations: stringArray(parsed.limitations),
    };
  } catch {
    return {
      format: null,
      capture: {
        mode: null,
        reason: null,
        fidelity: null,
        note: null,
      },
      target: {
        source: null,
        state: null,
        sessionId: null,
        pid: null,
        runtimeId: null,
        inFlight: null,
      },
      session: null,
      process: null,
      counts: {
        replayMessages: 0,
        debugCheckpoints: 0,
        toolInvocations: 0,
        toolAuditLines: 0,
        sessionAuditEvents: 0,
        workspaceEntries: 0,
        limitations: 0,
      },
      limitations: [],
    };
  }
}

function recordValue(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function optionalRecordValue(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function nullableStringValue(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function nullableNumberValue(value: unknown): number | null {
  return typeof value === "number" ? value : null;
}

function nullableBooleanValue(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function arrayLength(value: unknown): number {
  return Array.isArray(value) ? value.length : 0;
}

function stringArray(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.filter((item): item is string => typeof item === "string");
}
