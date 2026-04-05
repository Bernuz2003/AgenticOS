export interface ParsedCoreDumpCheckpoint {
  id: string;
  createdAtMs: number | null;
  boundaryKind: string | null;
  ordinal: number | null;
  payloadPreview: string | null;
}

export interface ParsedCoreDumpToolInvocation {
  id: string;
  createdAtMs: number | null;
  toolName: string;
  caller: string | null;
  transport: string | null;
  status: string | null;
  commandText: string | null;
  inputPreview: string | null;
  outputPreview: string | null;
  errorKind: string | null;
  durationMs: number | null;
  kill: boolean;
  warnings: string[];
}

export interface ParsedCoreDumpAuditEvent {
  id: string;
  recordedAtMs: number | null;
  category: string | null;
  kind: string | null;
  title: string | null;
  detail: string | null;
  pid: number | null;
}

export interface ParsedCoreDumpReplayMessage {
  id: string;
  createdAtMs: number | null;
  role: string | null;
  kind: string | null;
  content: string | null;
}

export interface ParsedCoreDumpContextSegment {
  id: string;
  kind: string | null;
  text: string | null;
  score: number | null;
  chars: number | null;
  tokens: number | null;
}

export interface ParsedCoreDumpWorkspaceEntry {
  id: string;
  path: string | null;
  kind: string | null;
  bytes: number | null;
  sha256: string | null;
}

export interface ParsedCoreDumpManifest {
  raw: Record<string, unknown> | null;
  debugCheckpoints: ParsedCoreDumpCheckpoint[];
  toolInvocations: ParsedCoreDumpToolInvocation[];
  auditEvents: ParsedCoreDumpAuditEvent[];
  replayMessages: ParsedCoreDumpReplayMessage[];
  contextSegments: ParsedCoreDumpContextSegment[];
  episodicSegments: ParsedCoreDumpContextSegment[];
  workspaceEntries: ParsedCoreDumpWorkspaceEntry[];
  limitations: string[];
}

export function parseCoreDumpManifest(manifestJson: string): ParsedCoreDumpManifest {
  try {
    const parsed = JSON.parse(manifestJson) as Record<string, unknown>;
    const workspace = optionalRecord(parsed.workspace);

    return {
      raw: parsed,
      debugCheckpoints: arrayRecords(parsed.debug_checkpoints).map((checkpoint, index) => ({
        id: stringValue(checkpoint.checkpoint_id) ?? `checkpoint-${index}`,
        createdAtMs:
          numberValue(checkpoint.created_at_ms) ?? numberValue(checkpoint.recorded_at_ms),
        boundaryKind: stringValue(checkpoint.boundary_kind),
        ordinal: numberValue(checkpoint.ordinal),
        payloadPreview: previewValue(checkpoint.payload_json),
      })),
      toolInvocations: arrayRecords(
        parsed.tool_invocation_history ?? parsed.tool_invocations,
      ).map((entry, index) => ({
        id:
          stringValue(entry.tool_call_id) ??
          stringValue(entry.invocation_id) ??
          `tool-call-${index}`,
        createdAtMs:
          numberValue(entry.recorded_at_ms) ??
          numberValue(entry.created_at_ms) ??
          numberValue(entry.updated_at_ms),
        toolName: stringValue(entry.tool_name) ?? "unknown_tool",
        caller: stringValue(entry.caller),
        transport: stringValue(entry.transport),
        status: stringValue(entry.status),
        commandText: stringValue(entry.command_text) ?? stringValue(entry.display_text),
        inputPreview: previewValue(entry.input_json),
        outputPreview: stringValue(entry.output_text) ?? previewValue(entry.output_json),
        errorKind: stringValue(entry.error_kind),
        durationMs: numberValue(entry.duration_ms),
        kill: booleanValue(entry.kill) ?? false,
        warnings: stringArray(entry.warnings_json ?? entry.warnings),
      })),
      auditEvents: arrayRecords(parsed.session_audit_events).map((entry, index) => ({
        id: `${stringValue(entry.category) ?? "audit"}-${index}`,
        recordedAtMs:
          numberValue(entry.recorded_at_ms) ?? numberValue(entry.created_at_ms),
        category: stringValue(entry.category),
        kind: stringValue(entry.kind),
        title: stringValue(entry.title) ?? stringValue(entry.kind),
        detail: stringValue(entry.detail),
        pid: numberValue(entry.pid),
      })),
      replayMessages: arrayRecords(parsed.replay_messages).map((entry, index) => ({
        id: `${stringValue(entry.role) ?? "message"}-${index}`,
        createdAtMs:
          numberValue(entry.created_at_ms) ?? numberValue(entry.recorded_at_ms),
        role: stringValue(entry.role),
        kind: stringValue(entry.kind),
        content: stringValue(entry.content),
      })),
      contextSegments: arrayRecords(parsed.context_segments).map((entry, index) =>
        mapContextSegment(entry, index),
      ),
      episodicSegments: arrayRecords(parsed.episodic_segments).map((entry, index) =>
        mapContextSegment(entry, index),
      ),
      workspaceEntries: arrayRecords(workspace?.entries).map((entry, index) => ({
        id: `workspace-${index}`,
        path: stringValue(entry.path),
        kind: stringValue(entry.kind),
        bytes: numberValue(entry.bytes),
        sha256: stringValue(entry.sha256),
      })),
      limitations: stringArray(parsed.limitations),
    };
  } catch {
    return {
      raw: null,
      debugCheckpoints: [],
      toolInvocations: [],
      auditEvents: [],
      replayMessages: [],
      contextSegments: [],
      episodicSegments: [],
      workspaceEntries: [],
      limitations: [],
    };
  }
}

function mapContextSegment(
  entry: Record<string, unknown>,
  index: number,
): ParsedCoreDumpContextSegment {
  return {
    id: `segment-${index}`,
    kind: stringValue(entry.kind),
    text: stringValue(entry.text),
    score: numberValue(entry.score),
    chars: numberValue(entry.chars),
    tokens: numberValue(entry.tokens),
  };
}

function optionalRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function arrayRecords(value: unknown): Record<string, unknown>[] {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.filter(
    (entry): entry is Record<string, unknown> =>
      entry !== null && typeof entry === "object" && !Array.isArray(entry),
  );
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function numberValue(value: unknown): number | null {
  return typeof value === "number" ? value : null;
}

function booleanValue(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function stringArray(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value.filter((item): item is string => typeof item === "string");
  }
  if (typeof value === "string") {
    return [value];
  }
  return [];
}

function previewValue(value: unknown): string | null {
  if (typeof value === "string") {
    return value;
  }
  if (value === null || value === undefined) {
    return null;
  }
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}
