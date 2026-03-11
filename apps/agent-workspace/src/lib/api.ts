import { invoke } from "@tauri-apps/api/core";

export interface LobbySnapshotSession {
  sessionId: string;
  pid: number;
  title: string;
  promptPreview: string;
  status: string;
  uptimeLabel: string;
  tokensLabel: string;
  contextStrategy: string;
  orchestrationId: number | null;
  orchestrationTaskId: string | null;
}

export interface LobbySnapshot {
  connected: boolean;
  selectedModelId: string;
  loadedModelId: string;
  orchestrations: LobbyOrchestrationSummary[];
  sessions: LobbySnapshotSession[];
  error: string | null;
}

export interface LobbyOrchestrationSummary {
  orchestrationId: number;
  total: number;
  completed: number;
  running: number;
  pending: number;
  failed: number;
  skipped: number;
  finished: boolean;
  elapsedLabel: string;
  policy: string;
}

export interface WorkspaceContextSnapshot {
  contextStrategy: string;
  contextTokensUsed: number;
  contextWindowSize: number;
  contextCompressions: number;
  contextRetrievalHits: number;
  lastCompactionReason: string | null;
  lastSummaryTs: string | null;
  contextSegments: number;
}

export interface WorkspaceSnapshot {
  sessionId: string;
  pid: number;
  state: string;
  workload: string;
  tokensGenerated: number;
  syscallsUsed: number;
  elapsedSecs: number;
  tokens: number;
  maxTokens: number;
  orchestration: WorkspaceOrchestrationSnapshot | null;
  context: WorkspaceContextSnapshot | null;
  auditEvents: AuditEvent[];
}

export interface WorkspaceOrchestrationSnapshot {
  orchestrationId: number;
  taskId: string;
  total: number;
  completed: number;
  running: number;
  pending: number;
  failed: number;
  skipped: number;
  finished: boolean;
  elapsedSecs: number;
  policy: string;
  tasks: Array<{
    task: string;
    status: string;
    pid: number | null;
  }>;
}

export interface AuditEvent {
  category: string;
  title: string;
  detail: string;
}

export interface StartSessionResult {
  sessionId: string;
  pid: number;
}

export interface OrchestrateResult {
  orchestrationId: number;
  totalTasks: number;
  spawned: number;
}

export interface ModelCatalogEntry {
  id: string;
  family: string;
  architecture: string | null;
  path: string;
  tokenizerPath: string | null;
  tokenizerPresent: boolean;
  metadataSource: string | null;
  backendPreference: string | null;
  resolvedBackend: string | null;
  driverResolutionSource: string;
  driverResolutionRationale: string;
  driverAvailable: boolean | null;
  driverLoadSupported: boolean | null;
  capabilities: Record<string, number> | null;
  selected: boolean;
}

export interface ModelRoutingRecommendation {
  workload: string;
  modelId: string | null;
  family: string | null;
  backendPreference: string | null;
  resolvedBackend: string | null;
  driverResolutionSource: string;
  driverResolutionRationale: string;
  driverAvailable: boolean | null;
  driverLoadSupported: boolean | null;
  metadataSource: string | null;
  source: string;
  rationale: string;
  capabilityKey: string | null;
  capabilityScore: number | null;
}

export interface ModelCatalogSnapshot {
  selectedModelId: string | null;
  totalModels: number;
  models: ModelCatalogEntry[];
  routingRecommendations: ModelRoutingRecommendation[];
}

export interface SelectModelResult {
  selectedModel: string;
}

export interface LoadModelResult {
  family: string;
  backend: string;
  driverSource: string;
  driverRationale: string;
  path: string;
  architecture: string | null;
  loadMode: string;
}

export interface SendInputResult {
  pid: number;
  state: string;
}

export interface TurnControlResult {
  pid: number;
  state: string;
  action: string;
}

export type TimelineItemKind =
  | "user_message"
  | "thinking"
  | "tool_call"
  | "tool_result"
  | "assistant_message"
  | "system_event";

export interface TimelineItem {
  id: string;
  kind: TimelineItemKind;
  text: string;
  status: string;
}

export interface TimelineSnapshot {
  sessionId: string;
  pid: number;
  running: boolean;
  workload: string;
  source: string;
  fallbackNotice: string | null;
  error: string | null;
  items: TimelineItem[];
}

export interface LobbySnapshotDto {
  connected: boolean;
  selected_model_id: string;
  loaded_model_id: string;
  orchestrations: Array<{
    orchestration_id: number;
    total: number;
    completed: number;
    running: number;
    pending: number;
    failed: number;
    skipped: number;
    finished: boolean;
    elapsed_label: string;
    policy: string;
  }>;
  sessions: Array<{
    session_id: string;
    pid: number;
    title: string;
    prompt_preview: string;
    status: string;
    uptime_label: string;
    tokens_label: string;
    context_strategy?: string | null;
    orchestration_id: number | null;
    orchestration_task_id: string | null;
  }>;
  error: string | null;
}

export interface WorkspaceSnapshotDto {
  session_id: string;
  pid: number;
  state: string;
  workload: string;
  tokens_generated: number;
  syscalls_used: number;
  elapsed_secs: number;
  tokens: number;
  max_tokens: number;
  orchestration: null | {
    orchestration_id: number;
    task_id: string;
    total: number;
    completed: number;
    running: number;
    pending: number;
    failed: number;
    skipped: number;
    finished: boolean;
    elapsed_secs: number;
    policy: string;
    tasks: Array<{
      task: string;
      status: string;
      pid: number | null;
    }>;
  };
  context: null | {
    context_strategy: string;
    context_tokens_used: number;
    context_window_size: number;
    context_compressions: number;
    context_retrieval_hits: number;
    last_compaction_reason: string | null;
    last_summary_ts: string | null;
    context_segments: number;
  };
  audit_events: Array<{
    category: string;
    title: string;
    detail: string;
  }>;
}

export interface TimelineSnapshotDto {
  session_id: string;
  pid: number;
  running: boolean;
  workload: string;
  source: string;
  fallback_notice: string | null;
  error: string | null;
  items: Array<{
    id: string;
    kind: TimelineItemKind;
    text: string;
    status: string;
  }>;
}

export function normalizeLobbySnapshot(snapshot: LobbySnapshotDto): LobbySnapshot {
  return {
    connected: snapshot.connected,
    selectedModelId: snapshot.selected_model_id,
    loadedModelId: snapshot.loaded_model_id,
    orchestrations: snapshot.orchestrations.map((orchestration) => ({
      orchestrationId: orchestration.orchestration_id,
      total: orchestration.total,
      completed: orchestration.completed,
      running: orchestration.running,
      pending: orchestration.pending,
      failed: orchestration.failed,
      skipped: orchestration.skipped,
      finished: orchestration.finished,
      elapsedLabel: orchestration.elapsed_label,
      policy: orchestration.policy,
    })),
    error: snapshot.error,
    sessions: snapshot.sessions.map((session) => ({
      sessionId: session.session_id,
      pid: session.pid,
      title: session.title,
      promptPreview: session.prompt_preview,
      status: session.status,
      uptimeLabel: session.uptime_label,
      tokensLabel: session.tokens_label,
      contextStrategy: session.context_strategy ?? "sliding_window",
      orchestrationId: session.orchestration_id,
      orchestrationTaskId: session.orchestration_task_id,
    })),
  };
}

export function normalizeWorkspaceSnapshot(snapshot: WorkspaceSnapshotDto): WorkspaceSnapshot {
  return {
    sessionId: snapshot.session_id,
    pid: snapshot.pid,
    state: snapshot.state,
    workload: snapshot.workload,
    tokensGenerated: snapshot.tokens_generated,
    syscallsUsed: snapshot.syscalls_used,
    elapsedSecs: snapshot.elapsed_secs,
    tokens: snapshot.tokens,
    maxTokens: snapshot.max_tokens,
    orchestration: snapshot.orchestration
      ? {
          orchestrationId: snapshot.orchestration.orchestration_id,
          taskId: snapshot.orchestration.task_id,
          total: snapshot.orchestration.total,
          completed: snapshot.orchestration.completed,
          running: snapshot.orchestration.running,
          pending: snapshot.orchestration.pending,
          failed: snapshot.orchestration.failed,
          skipped: snapshot.orchestration.skipped,
          finished: snapshot.orchestration.finished,
          elapsedSecs: snapshot.orchestration.elapsed_secs,
          policy: snapshot.orchestration.policy,
          tasks: snapshot.orchestration.tasks.map((task) => ({
            task: task.task,
            status: task.status,
            pid: task.pid,
          })),
        }
      : null,
    context: snapshot.context
      ? {
          contextStrategy: snapshot.context.context_strategy,
          contextTokensUsed: snapshot.context.context_tokens_used,
          contextWindowSize: snapshot.context.context_window_size,
          contextCompressions: snapshot.context.context_compressions,
          contextRetrievalHits: snapshot.context.context_retrieval_hits,
          lastCompactionReason: snapshot.context.last_compaction_reason,
          lastSummaryTs: snapshot.context.last_summary_ts,
          contextSegments: snapshot.context.context_segments,
        }
      : null,
    auditEvents: snapshot.audit_events.map((event) => ({
      category: event.category,
      title: event.title,
      detail: event.detail,
    })),
  };
}

export function normalizeTimelineSnapshot(snapshot: TimelineSnapshotDto): TimelineSnapshot {
  return {
    sessionId: snapshot.session_id,
    pid: snapshot.pid,
    running: snapshot.running,
    workload: snapshot.workload,
    source: snapshot.source,
    fallbackNotice: snapshot.fallback_notice,
    error: snapshot.error,
    items: snapshot.items,
  };
}

export async function fetchLobbySnapshot(): Promise<LobbySnapshot> {
  const snapshot = await invoke<LobbySnapshotDto>("fetch_lobby_snapshot");
  return normalizeLobbySnapshot(snapshot);
}

export async function fetchWorkspaceSnapshot(pid: number): Promise<WorkspaceSnapshot> {
  const snapshot = await invoke<WorkspaceSnapshotDto>("fetch_workspace_snapshot", { pid });
  return normalizeWorkspaceSnapshot(snapshot);
}

export async function startSession(
  prompt: string,
  workload: string,
): Promise<StartSessionResult> {
  const session = await invoke<{
    session_id: string;
    pid: number;
  }>("start_session", { prompt, workload });

  return {
    sessionId: session.session_id,
    pid: session.pid,
  };
}

export async function orchestrate(payload: string): Promise<OrchestrateResult> {
  const result = await invoke<{
    orchestration_id: number;
    total_tasks: number;
    spawned: number;
  }>("orchestrate", { payload });

  return {
    orchestrationId: result.orchestration_id,
    totalTasks: result.total_tasks,
    spawned: result.spawned,
  };
}

export async function pingKernel(): Promise<string> {
  return invoke<string>("ping_kernel");
}

export async function listModels(): Promise<ModelCatalogSnapshot> {
  const snapshot = await invoke<{
    selected_model_id: string | null;
    total_models: number;
    models: Array<{
      id: string;
      family: string;
      architecture: string | null;
      path: string;
      tokenizer_path: string | null;
      tokenizer_present: boolean;
      metadata_source: string | null;
      backend_preference: string | null;
      resolved_backend: string | null;
      driver_resolution_source: string;
      driver_resolution_rationale: string;
      driver_available: boolean | null;
      driver_load_supported: boolean | null;
      capabilities: Record<string, number> | null;
      selected: boolean;
    }>;
    routing_recommendations: Array<{
      workload: string;
      model_id: string | null;
      family: string | null;
      backend_preference: string | null;
      resolved_backend: string | null;
      driver_resolution_source: string;
      driver_resolution_rationale: string;
      driver_available: boolean | null;
      driver_load_supported: boolean | null;
      metadata_source: string | null;
      source: string;
      rationale: string;
      capability_key: string | null;
      capability_score: number | null;
    }>;
  }>("list_models");

  return {
    selectedModelId: snapshot.selected_model_id,
    totalModels: snapshot.total_models,
    models: snapshot.models.map((model) => ({
      id: model.id,
      family: model.family,
      architecture: model.architecture,
      path: model.path,
      tokenizerPath: model.tokenizer_path,
      tokenizerPresent: model.tokenizer_present,
      metadataSource: model.metadata_source,
      backendPreference: model.backend_preference,
      resolvedBackend: model.resolved_backend,
      driverResolutionSource: model.driver_resolution_source,
      driverResolutionRationale: model.driver_resolution_rationale,
      driverAvailable: model.driver_available,
      driverLoadSupported: model.driver_load_supported,
      capabilities: model.capabilities,
      selected: model.selected,
    })),
    routingRecommendations: snapshot.routing_recommendations.map((entry) => ({
      workload: entry.workload,
      modelId: entry.model_id,
      family: entry.family,
      backendPreference: entry.backend_preference,
      resolvedBackend: entry.resolved_backend,
      driverResolutionSource: entry.driver_resolution_source,
      driverResolutionRationale: entry.driver_resolution_rationale,
      driverAvailable: entry.driver_available,
      driverLoadSupported: entry.driver_load_supported,
      metadataSource: entry.metadata_source,
      source: entry.source,
      rationale: entry.rationale,
      capabilityKey: entry.capability_key,
      capabilityScore: entry.capability_score,
    })),
  };
}

export async function selectModel(modelId: string): Promise<SelectModelResult> {
  const result = await invoke<{ selected_model: string }>("select_model", {
    modelId,
  });

  return {
    selectedModel: result.selected_model,
  };
}

export async function loadModel(selector = ""): Promise<LoadModelResult> {
  const result = await invoke<{
    family: string;
    backend: string;
    driver_source: string;
    driver_rationale: string;
    path: string;
    architecture: string | null;
    load_mode: string;
  }>("load_model", { selector });

  return {
    family: result.family,
    backend: result.backend,
    driverSource: result.driver_source,
    driverRationale: result.driver_rationale,
    path: result.path,
    architecture: result.architecture,
    loadMode: result.load_mode,
  };
}

export async function sendSessionInput(
  pid: number,
  prompt: string,
): Promise<SendInputResult> {
  const result = await invoke<{
    pid: number;
    state: string;
  }>("send_session_input", { pid, prompt });

  return {
    pid: result.pid,
    state: result.state,
  };
}

export async function continueSessionOutput(pid: number): Promise<TurnControlResult> {
  const result = await invoke<{
    pid: number;
    state: string;
    action: string;
  }>("continue_session_output", { pid });

  return {
    pid: result.pid,
    state: result.state,
    action: result.action,
  };
}

export async function stopSessionOutput(pid: number): Promise<TurnControlResult> {
  const result = await invoke<{
    pid: number;
    state: string;
    action: string;
  }>("stop_session_output", { pid });

  return {
    pid: result.pid,
    state: result.state,
    action: result.action,
  };
}

export async function shutdownKernel(): Promise<string> {
  return invoke<string>("shutdown_kernel");
}

export async function fetchTimelineSnapshot(pid: number): Promise<TimelineSnapshot> {
  const snapshot = await invoke<TimelineSnapshotDto>("fetch_timeline_snapshot", { pid });
  return normalizeTimelineSnapshot(snapshot);
}
