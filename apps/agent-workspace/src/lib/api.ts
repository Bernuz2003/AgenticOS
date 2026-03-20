import { invoke } from "@tauri-apps/api/core";

export interface LobbySnapshotSession {
  sessionId: string;
  pid: number;
  activePid: number | null;
  lastPid: number | null;
  title: string;
  promptPreview: string;
  status: string;
  runtimeState: string | null;
  uptimeLabel: string;
  tokensLabel: string;
  contextStrategy: string;
  runtimeId: string | null;
  runtimeLabel: string | null;
  backendClass: string | null;
  orchestrationId: number | null;
  orchestrationTaskId: string | null;
}

export interface LobbySnapshot {
  connected: boolean;
  selectedModelId: string;
  loadedModelId: string;
  loadedTargetKind: string | null;
  loadedProviderId: string | null;
  loadedRemoteModelId: string | null;
  loadedBackendId: string | null;
  loadedBackendClass: string | null;
  loadedBackendCapabilities: BackendCapabilities | null;
  globalAccounting: BackendTelemetry | null;
  loadedBackendTelemetry: BackendTelemetry | null;
  loadedRemoteModel: RemoteRuntimeModel | null;
  memory: MemoryStatus | null;
  runtimeInstances: RuntimeInstance[];
  resourceGovernor: ResourceGovernorStatus | null;
  runtimeLoadQueue: RuntimeLoadQueueEntry[];
  globalAuditEvents: AuditEvent[];
  scheduledJobs: ScheduledJob[];
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
  contextRetrievalRequests: number;
  contextRetrievalMisses: number;
  contextRetrievalCandidatesScored: number;
  contextRetrievalSegmentsSelected: number;
  lastRetrievalCandidatesScored: number;
  lastRetrievalSegmentsSelected: number;
  lastRetrievalLatencyMs: number;
  lastRetrievalTopScore: number | null;
  lastCompactionReason: string | null;
  lastSummaryTs: string | null;
  contextSegments: number;
  episodicSegments: number;
  episodicTokens: number;
  retrieveTopK: number;
  retrieveCandidateLimit: number;
  retrieveMaxSegmentChars: number;
  retrieveMinScore: number;
}

export interface HumanInputRequest {
  kind: string;
  question: string;
  details: string | null;
  choices: string[];
  allowFreeText: boolean;
  placeholder: string | null;
  requestedAtMs: number;
}

export interface WorkspaceSnapshot {
  sessionId: string;
  pid: number;
  activePid: number | null;
  lastPid: number | null;
  title: string;
  runtimeId: string | null;
  runtimeLabel: string | null;
  state: string;
  workload: string;
  ownerId: number | null;
  toolCaller: string | null;
  indexPos: number | null;
  priority: string | null;
  quotaTokens: number | null;
  quotaSyscalls: number | null;
  contextSlotId: number | null;
  residentSlotPolicy: string | null;
  residentSlotState: string | null;
  residentSlotSnapshotPath: string | null;
  backendId: string | null;
  backendClass: string | null;
  backendCapabilities: BackendCapabilities | null;
  accounting: BackendTelemetry | null;
  permissions: ProcessPermissions | null;
  tokensGenerated: number;
  syscallsUsed: number;
  elapsedSecs: number;
  tokens: number;
  maxTokens: number;
  orchestration: WorkspaceOrchestrationSnapshot | null;
  context: WorkspaceContextSnapshot | null;
  pendingHumanRequest: HumanInputRequest | null;
  auditEvents: AuditEvent[];
}

export interface ProcessPermissions {
  trustScope: string;
  actionsAllowed: boolean;
  allowedTools: string[];
  pathScopes: string[];
}

export interface MemoryStatus {
  active: boolean;
  totalBlocks: number;
  freeBlocks: number;
  trackedPids: number;
  allocatedTensors: number;
  allocBytes: number;
  evictions: number;
  swapCount: number;
  swapFaults: number;
  swapFailures: number;
  pendingSwaps: number;
  parkedPids: number;
  oomEvents: number;
  swapWorkerCrashes: number;
}

export interface RuntimeInstance {
  runtimeId: string;
  targetKind: string;
  logicalModelId: string;
  displayPath: string;
  family: string;
  backendId: string;
  backendClass: string;
  providerId: string | null;
  remoteModelId: string | null;
  state: string;
  reservationRamBytes: number;
  reservationVramBytes: number;
  pinned: boolean;
  transitionState: string | null;
  activePidCount: number;
  activePids: number[];
  current: boolean;
}

export interface RuntimeLoadQueueEntry {
  queueId: number;
  logicalModelId: string;
  displayPath: string;
  backendClass: string;
  state: string;
  reservationRamBytes: number;
  reservationVramBytes: number;
  reason: string;
  requestedAtMs: number;
  updatedAtMs: number;
}

export interface ResourceGovernorStatus {
  ramBudgetBytes: number;
  vramBudgetBytes: number;
  minRamHeadroomBytes: number;
  minVramHeadroomBytes: number;
  ramUsedBytes: number;
  vramUsedBytes: number;
  ramAvailableBytes: number;
  vramAvailableBytes: number;
  pendingQueueDepth: number;
  loaderBusy: boolean;
  loaderReason: string | null;
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
  kind: string;
  title: string;
  detail: string;
  recordedAtMs: number;
  sessionId: string | null;
  pid: number | null;
  runtimeId: string | null;
}

export interface AuditEventDto {
  category: string;
  kind: string;
  title: string;
  detail: string;
  recorded_at_ms: number;
  session_id: string | null;
  pid: number | null;
  runtime_id: string | null;
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

export interface RetryWorkflowTaskResult {
  orchestrationId: number;
  task: string;
  resetTasks: string[];
  spawned: number;
}

export interface WorkflowRunControlResult {
  orchestrationId: number;
  status: string;
}

export interface ScheduleJobResult {
  jobId: number;
  nextRunAtMs: number | null;
  triggerKind: string;
}

export interface ScheduledJobControlResult {
  jobId: number;
  enabled: boolean;
  state: string;
}

export interface ScheduledJobRun {
  runId: number;
  triggerAtMs: number;
  attempt: number;
  status: string;
  startedAtMs: number | null;
  completedAtMs: number | null;
  orchestrationId: number | null;
  deadlineAtMs: number | null;
  error: string | null;
}

export interface ScheduledJob {
  jobId: number;
  name: string;
  targetKind: string;
  triggerKind: string;
  triggerLabel: string;
  enabled: boolean;
  state: string;
  nextRunAtMs: number | null;
  currentTriggerAtMs: number | null;
  currentAttempt: number;
  timeoutMs: number;
  maxRetries: number;
  backoffMs: number;
  lastRunStartedAtMs: number | null;
  lastRunCompletedAtMs: number | null;
  lastRunStatus: string | null;
  lastError: string | null;
  consecutiveFailures: number;
  activeOrchestrationId: number | null;
  recentRuns: ScheduledJobRun[];
}

export interface OrchestrationArtifactRef {
  artifactId: string;
  task: string;
  attempt: number;
  kind: string;
  label: string;
}

export interface OrchestrationArtifact {
  artifactId: string;
  task: string;
  attempt: number;
  kind: string;
  label: string;
  mimeType: string;
  preview: string;
  content: string;
  bytes: number;
  createdAtMs: number;
}

export interface OrchestrationTaskAttempt {
  attempt: number;
  status: string;
  sessionId: string | null;
  pid: number | null;
  error: string | null;
  outputPreview: string;
  outputChars: number;
  truncated: boolean;
  startedAtMs: number;
  completedAtMs: number | null;
  primaryArtifactId: string | null;
}

export interface OrchestrationTaskStatus {
  task: string;
  role: string | null;
  workload: string | null;
  backendClass: string | null;
  contextStrategy: string | null;
  deps: string[];
  status: string;
  currentAttempt: number | null;
  pid: number | null;
  error: string | null;
  context: WorkspaceContextSnapshot | null;
  latestOutputPreview: string | null;
  latestOutputText: string | null;
  latestOutputTruncated: boolean;
  inputArtifacts: OrchestrationArtifactRef[];
  outputArtifacts: OrchestrationArtifact[];
  attempts: OrchestrationTaskAttempt[];
}

export interface OrchestrationStatus {
  orchestrationId: number;
  total: number;
  completed: number;
  running: number;
  pending: number;
  failed: number;
  skipped: number;
  finished: boolean;
  elapsedSecs: number;
  policy: string;
  truncations: number;
  outputCharsStored: number;
  tasks: OrchestrationTaskStatus[];
}

export interface BackendCapabilities {
  residentKv: boolean;
  persistentSlots: boolean;
  saveRestoreSlots: boolean;
  promptCacheReuse: boolean;
  streamingGeneration: boolean;
  structuredOutput: boolean;
  cancelGeneration: boolean;
  memoryTelemetry: boolean;
  toolPauseResume: boolean;
  contextCompactionReset: boolean;
  parallelSessions: boolean;
}

export interface BackendTelemetry {
  requestsTotal: number;
  streamRequestsTotal: number;
  inputTokensTotal: number;
  outputTokensTotal: number;
  estimatedCostUsd: number;
  rateLimitErrors: number;
  authErrors: number;
  transportErrors: number;
  lastModel: string | null;
  lastError: string | null;
}

export interface RemoteRuntimeModel {
  providerId: string;
  providerLabel: string;
  backendId: string;
  adapterKind: string;
  modelId: string;
  modelLabel: string;
  contextWindowTokens: number | null;
  maxOutputTokens: number | null;
  supportsStructuredOutput: boolean;
  inputPriceUsdPerMtok: number | null;
  outputPriceUsdPerMtok: number | null;
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
  resolvedBackendClass: string | null;
  resolvedBackendCapabilities: BackendCapabilities | null;
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
  resolvedBackendClass: string | null;
  resolvedBackendCapabilities: BackendCapabilities | null;
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

export interface RemoteProviderModel {
  id: string;
  label: string;
  contextWindowTokens: number | null;
  maxOutputTokens: number | null;
  supportsStructuredOutput: boolean;
  inputPriceUsdPerMtok: number | null;
  outputPriceUsdPerMtok: number | null;
}

export interface RemoteProvider {
  id: string;
  backendId: string;
  adapterKind: string;
  label: string;
  note: string | null;
  credentialHint: string | null;
  defaultModelId: string;
  models: RemoteProviderModel[];
}

export interface ModelCatalogSnapshot {
  selectedModelId: string | null;
  totalModels: number;
  models: ModelCatalogEntry[];
  routingRecommendations: ModelRoutingRecommendation[];
  remoteProviders: RemoteProvider[];
}

export interface SelectModelResult {
  selectedModel: string;
}

export interface LoadModelResult {
  family: string;
  loadedModelId: string;
  loadedTargetKind: string;
  loadedProviderId: string | null;
  loadedRemoteModelId: string | null;
  backend: string;
  backendClass: string;
  backendCapabilities: BackendCapabilities;
  driverSource: string;
  driverRationale: string;
  path: string;
  architecture: string | null;
  loadMode: string;
  remoteModel: RemoteRuntimeModel | null;
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
  | "action_call"
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
  loaded_target_kind: string | null;
  loaded_provider_id: string | null;
  loaded_remote_model_id: string | null;
  loaded_backend_id: string | null;
  loaded_backend_class: string | null;
  loaded_backend_capabilities: {
    resident_kv: boolean;
    persistent_slots: boolean;
    save_restore_slots: boolean;
    prompt_cache_reuse: boolean;
    streaming_generation: boolean;
    structured_output: boolean;
    cancel_generation: boolean;
    memory_telemetry: boolean;
    tool_pause_resume: boolean;
    context_compaction_reset: boolean;
    parallel_sessions: boolean;
  } | null;
  global_accounting: {
    requests_total: number;
    stream_requests_total: number;
    input_tokens_total: number;
    output_tokens_total: number;
    estimated_cost_usd: number;
    rate_limit_errors: number;
    auth_errors: number;
    transport_errors: number;
    last_model: string | null;
    last_error: string | null;
  } | null;
  loaded_backend_telemetry: {
    requests_total: number;
    stream_requests_total: number;
    input_tokens_total: number;
    output_tokens_total: number;
    estimated_cost_usd: number;
    rate_limit_errors: number;
    auth_errors: number;
    transport_errors: number;
    last_model: string | null;
    last_error: string | null;
  } | null;
  loaded_remote_model: {
    provider_id: string;
    provider_label: string;
    backend_id: string;
    adapter_kind: string;
    model_id: string;
    model_label: string;
    context_window_tokens: number | null;
    max_output_tokens: number | null;
    supports_structured_output: boolean;
    input_price_usd_per_mtok: number | null;
    output_price_usd_per_mtok: number | null;
  } | null;
  memory: {
    active: boolean;
    total_blocks: number;
    free_blocks: number;
    tracked_pids: number;
    allocated_tensors: number;
    alloc_bytes: number;
    evictions: number;
    swap_count: number;
    swap_faults: number;
    swap_failures: number;
    pending_swaps: number;
    parked_pids: number;
    oom_events: number;
    swap_worker_crashes: number;
  } | null;
  runtime_instances: Array<{
    runtime_id: string;
    target_kind: string;
    logical_model_id: string;
    display_path: string;
    family: string;
    backend_id: string;
    backend_class: string;
    provider_id: string | null;
    remote_model_id: string | null;
    state: string;
    reservation_ram_bytes: number;
    reservation_vram_bytes: number;
    pinned: boolean;
    transition_state: string | null;
    active_pid_count: number;
    active_pids: number[];
    current: boolean;
  }>;
  resource_governor: {
    ram_budget_bytes: number;
    vram_budget_bytes: number;
    min_ram_headroom_bytes: number;
    min_vram_headroom_bytes: number;
    ram_used_bytes: number;
    vram_used_bytes: number;
    ram_available_bytes: number;
    vram_available_bytes: number;
    pending_queue_depth: number;
    loader_busy: boolean;
    loader_reason: string | null;
  } | null;
  runtime_load_queue: Array<{
    queue_id: number;
    logical_model_id: string;
    display_path: string;
    backend_class: string;
    state: string;
    reservation_ram_bytes: number;
    reservation_vram_bytes: number;
    reason: string;
    requested_at_ms: number;
    updated_at_ms: number;
  }>;
  global_audit_events: AuditEventDto[];
  scheduled_jobs: Array<{
    job_id: number;
    name: string;
    target_kind: string;
    trigger_kind: string;
    trigger_label: string;
    enabled: boolean;
    state: string;
    next_run_at_ms: number | null;
    current_trigger_at_ms: number | null;
    current_attempt: number;
    timeout_ms: number;
    max_retries: number;
    backoff_ms: number;
    last_run_started_at_ms: number | null;
    last_run_completed_at_ms: number | null;
    last_run_status: string | null;
    last_error: string | null;
    consecutive_failures: number;
    active_orchestration_id: number | null;
    recent_runs: Array<{
      run_id: number;
      trigger_at_ms: number;
      attempt: number;
      status: string;
      started_at_ms: number | null;
      completed_at_ms: number | null;
      orchestration_id: number | null;
      deadline_at_ms: number | null;
      error: string | null;
    }>;
  }>;
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
    active_pid: number | null;
    last_pid: number | null;
    title: string;
    prompt_preview: string;
    status: string;
    runtime_state: string | null;
    uptime_label: string;
    tokens_label: string;
    context_strategy?: string | null;
    runtime_id: string | null;
    runtime_label: string | null;
    backend_class: string | null;
    orchestration_id: number | null;
    orchestration_task_id: string | null;
  }>;
  error: string | null;
}

export interface WorkspaceSnapshotDto {
  session_id: string;
  pid: number;
  active_pid: number | null;
  last_pid: number | null;
  title: string;
  runtime_id: string | null;
  runtime_label: string | null;
  state: string;
  workload: string;
  owner_id: number | null;
  tool_caller: string | null;
  index_pos: number | null;
  priority: string | null;
  quota_tokens: number | null;
  quota_syscalls: number | null;
  context_slot_id: number | null;
  resident_slot_policy: string | null;
  resident_slot_state: string | null;
  resident_slot_snapshot_path: string | null;
  backend_id: string | null;
  backend_class: string | null;
  backend_capabilities: {
    resident_kv: boolean;
    persistent_slots: boolean;
    save_restore_slots: boolean;
    prompt_cache_reuse: boolean;
    streaming_generation: boolean;
    structured_output: boolean;
    cancel_generation: boolean;
    memory_telemetry: boolean;
    tool_pause_resume: boolean;
    context_compaction_reset: boolean;
    parallel_sessions: boolean;
  } | null;
  accounting: {
    requests_total: number;
    stream_requests_total: number;
    input_tokens_total: number;
    output_tokens_total: number;
    estimated_cost_usd: number;
    rate_limit_errors: number;
    auth_errors: number;
    transport_errors: number;
    last_model: string | null;
    last_error: string | null;
  } | null;
  permissions: {
    trust_scope: string;
    actions_allowed: boolean;
    allowed_tools: string[];
    path_scopes: string[];
  } | null;
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
    context_retrieval_requests: number;
    context_retrieval_misses: number;
    context_retrieval_candidates_scored: number;
    context_retrieval_segments_selected: number;
    last_retrieval_candidates_scored: number;
    last_retrieval_segments_selected: number;
    last_retrieval_latency_ms: number;
    last_retrieval_top_score: number | null;
    last_compaction_reason: string | null;
    last_summary_ts: string | null;
    context_segments: number;
    episodic_segments: number;
    episodic_tokens: number;
    retrieve_top_k: number;
    retrieve_candidate_limit: number;
    retrieve_max_segment_chars: number;
    retrieve_min_score: number;
  };
  pending_human_request: null | {
    kind: string;
    question: string;
    details: string | null;
    choices: string[];
    allow_free_text: boolean;
    placeholder: string | null;
    requested_at_ms: number;
  };
  audit_events: AuditEventDto[];
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
    loadedTargetKind: snapshot.loaded_target_kind,
    loadedProviderId: snapshot.loaded_provider_id,
    loadedRemoteModelId: snapshot.loaded_remote_model_id,
    loadedBackendId: snapshot.loaded_backend_id,
    loadedBackendClass: snapshot.loaded_backend_class,
    loadedBackendCapabilities: mapBackendCapabilities(
      snapshot.loaded_backend_capabilities,
    ),
    globalAccounting: mapBackendTelemetry(snapshot.global_accounting),
    loadedBackendTelemetry: mapBackendTelemetry(
      snapshot.loaded_backend_telemetry,
    ),
    loadedRemoteModel: mapRemoteRuntimeModel(snapshot.loaded_remote_model),
    memory: mapMemoryStatus(snapshot.memory),
    runtimeInstances: snapshot.runtime_instances.map(mapRuntimeInstance),
    resourceGovernor: mapResourceGovernor(snapshot.resource_governor),
    runtimeLoadQueue: snapshot.runtime_load_queue.map(mapRuntimeLoadQueueEntry),
    globalAuditEvents: snapshot.global_audit_events.map(normalizeAuditEvent),
    scheduledJobs: snapshot.scheduled_jobs.map(mapScheduledJob),
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
      activePid: session.active_pid,
      lastPid: session.last_pid,
      title: session.title,
      promptPreview: session.prompt_preview,
      status: session.status,
      runtimeState: session.runtime_state,
      uptimeLabel: session.uptime_label,
      tokensLabel: session.tokens_label,
      contextStrategy: session.context_strategy ?? "sliding_window",
      runtimeId: session.runtime_id,
      runtimeLabel: session.runtime_label,
      backendClass: session.backend_class,
      orchestrationId: session.orchestration_id,
      orchestrationTaskId: session.orchestration_task_id,
    })),
  };
}

export function normalizeAuditEvent(event: AuditEventDto): AuditEvent {
  return {
    category: event.category,
    kind: event.kind,
    title: event.title,
    detail: event.detail,
    recordedAtMs: event.recorded_at_ms,
    sessionId: event.session_id,
    pid: event.pid,
    runtimeId: event.runtime_id,
  };
}

export function auditEventKey(event: AuditEvent): string {
  return [
    event.recordedAtMs,
    event.category,
    event.kind,
    event.sessionId ?? "",
    event.pid ?? "",
    event.runtimeId ?? "",
    event.detail,
  ].join("|");
}

function formatElapsedLabel(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 1) {
    return "<1s";
  }
  if (seconds < 60) {
    return `${Math.round(seconds)}s`;
  }
  if (seconds < 3600) {
    return `${Math.floor(seconds / 60)}m`;
  }
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

function mapScheduledJob(job: LobbySnapshotDto["scheduled_jobs"][number]): ScheduledJob {
  return {
    jobId: job.job_id,
    name: job.name,
    targetKind: job.target_kind,
    triggerKind: job.trigger_kind,
    triggerLabel: job.trigger_label,
    enabled: job.enabled,
    state: job.state,
    nextRunAtMs: job.next_run_at_ms,
    currentTriggerAtMs: job.current_trigger_at_ms,
    currentAttempt: job.current_attempt,
    timeoutMs: job.timeout_ms,
    maxRetries: job.max_retries,
    backoffMs: job.backoff_ms,
    lastRunStartedAtMs: job.last_run_started_at_ms,
    lastRunCompletedAtMs: job.last_run_completed_at_ms,
    lastRunStatus: job.last_run_status,
    lastError: job.last_error,
    consecutiveFailures: job.consecutive_failures,
    activeOrchestrationId: job.active_orchestration_id,
    recentRuns: job.recent_runs.map((run) => ({
      runId: run.run_id,
      triggerAtMs: run.trigger_at_ms,
      attempt: run.attempt,
      status: run.status,
      startedAtMs: run.started_at_ms,
      completedAtMs: run.completed_at_ms,
      orchestrationId: run.orchestration_id,
      deadlineAtMs: run.deadline_at_ms,
      error: run.error,
    })),
  };
}

function mapRemoteRuntimeModel(
  model: LobbySnapshotDto["loaded_remote_model"],
): RemoteRuntimeModel | null {
  if (!model) {
    return null;
  }

  return {
    providerId: model.provider_id,
    providerLabel: model.provider_label,
    backendId: model.backend_id,
    adapterKind: model.adapter_kind,
    modelId: model.model_id,
    modelLabel: model.model_label,
    contextWindowTokens: model.context_window_tokens,
    maxOutputTokens: model.max_output_tokens,
    supportsStructuredOutput: model.supports_structured_output,
    inputPriceUsdPerMtok: model.input_price_usd_per_mtok,
    outputPriceUsdPerMtok: model.output_price_usd_per_mtok,
  };
}

function mapBackendTelemetry(
  telemetry: LobbySnapshotDto["loaded_backend_telemetry"],
): BackendTelemetry | null {
  if (!telemetry) {
    return null;
  }

  return {
    requestsTotal: telemetry.requests_total,
    streamRequestsTotal: telemetry.stream_requests_total,
    inputTokensTotal: telemetry.input_tokens_total,
    outputTokensTotal: telemetry.output_tokens_total,
    estimatedCostUsd: telemetry.estimated_cost_usd,
    rateLimitErrors: telemetry.rate_limit_errors,
    authErrors: telemetry.auth_errors,
    transportErrors: telemetry.transport_errors,
    lastModel: telemetry.last_model,
    lastError: telemetry.last_error,
  };
}

export function normalizeWorkspaceSnapshot(snapshot: WorkspaceSnapshotDto): WorkspaceSnapshot {
  return {
    sessionId: snapshot.session_id,
    pid: snapshot.pid,
    activePid: snapshot.active_pid,
    lastPid: snapshot.last_pid,
    title: snapshot.title,
    runtimeId: snapshot.runtime_id,
    runtimeLabel: snapshot.runtime_label,
    state: snapshot.state,
    workload: snapshot.workload,
    ownerId: snapshot.owner_id,
    toolCaller: snapshot.tool_caller,
    indexPos: snapshot.index_pos,
    priority: snapshot.priority,
    quotaTokens: snapshot.quota_tokens,
    quotaSyscalls: snapshot.quota_syscalls,
    contextSlotId: snapshot.context_slot_id,
    residentSlotPolicy: snapshot.resident_slot_policy,
    residentSlotState: snapshot.resident_slot_state,
    residentSlotSnapshotPath: snapshot.resident_slot_snapshot_path,
    backendId: snapshot.backend_id,
    backendClass: snapshot.backend_class,
    backendCapabilities: mapBackendCapabilities(snapshot.backend_capabilities),
    accounting: mapBackendTelemetry(snapshot.accounting),
    permissions: snapshot.permissions
      ? {
          trustScope: snapshot.permissions.trust_scope,
          actionsAllowed: snapshot.permissions.actions_allowed,
          allowedTools: snapshot.permissions.allowed_tools,
          pathScopes: snapshot.permissions.path_scopes,
        }
      : null,
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
          contextRetrievalRequests:
            snapshot.context.context_retrieval_requests,
          contextRetrievalMisses: snapshot.context.context_retrieval_misses,
          contextRetrievalCandidatesScored:
            snapshot.context.context_retrieval_candidates_scored,
          contextRetrievalSegmentsSelected:
            snapshot.context.context_retrieval_segments_selected,
          lastRetrievalCandidatesScored:
            snapshot.context.last_retrieval_candidates_scored,
          lastRetrievalSegmentsSelected:
            snapshot.context.last_retrieval_segments_selected,
          lastRetrievalLatencyMs: snapshot.context.last_retrieval_latency_ms,
          lastRetrievalTopScore: snapshot.context.last_retrieval_top_score,
          lastCompactionReason: snapshot.context.last_compaction_reason,
          lastSummaryTs: snapshot.context.last_summary_ts,
          contextSegments: snapshot.context.context_segments,
          episodicSegments: snapshot.context.episodic_segments,
          episodicTokens: snapshot.context.episodic_tokens,
          retrieveTopK: snapshot.context.retrieve_top_k,
          retrieveCandidateLimit:
            snapshot.context.retrieve_candidate_limit,
          retrieveMaxSegmentChars:
            snapshot.context.retrieve_max_segment_chars,
          retrieveMinScore: snapshot.context.retrieve_min_score,
        }
      : null,
    pendingHumanRequest: snapshot.pending_human_request
      ? {
          kind: snapshot.pending_human_request.kind,
          question: snapshot.pending_human_request.question,
          details: snapshot.pending_human_request.details,
          choices: snapshot.pending_human_request.choices,
          allowFreeText: snapshot.pending_human_request.allow_free_text,
          placeholder: snapshot.pending_human_request.placeholder,
          requestedAtMs: snapshot.pending_human_request.requested_at_ms,
        }
      : null,
    auditEvents: snapshot.audit_events.map(normalizeAuditEvent),
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

function mapBackendCapabilities(
  capabilities: {
    resident_kv: boolean;
    persistent_slots: boolean;
    save_restore_slots: boolean;
    prompt_cache_reuse: boolean;
    streaming_generation: boolean;
    structured_output: boolean;
    cancel_generation: boolean;
    memory_telemetry: boolean;
    tool_pause_resume: boolean;
    context_compaction_reset: boolean;
    parallel_sessions: boolean;
  } | null,
): BackendCapabilities | null {
  if (!capabilities) {
    return null;
  }

  return {
    residentKv: capabilities.resident_kv,
    persistentSlots: capabilities.persistent_slots,
    saveRestoreSlots: capabilities.save_restore_slots,
    promptCacheReuse: capabilities.prompt_cache_reuse,
    streamingGeneration: capabilities.streaming_generation,
    structuredOutput: capabilities.structured_output,
    cancelGeneration: capabilities.cancel_generation,
    memoryTelemetry: capabilities.memory_telemetry,
    toolPauseResume: capabilities.tool_pause_resume,
    contextCompactionReset: capabilities.context_compaction_reset,
    parallelSessions: capabilities.parallel_sessions,
  };
}

function mapMemoryStatus(
  memory: LobbySnapshotDto["memory"],
): MemoryStatus | null {
  if (!memory) {
    return null;
  }

  return {
    active: memory.active,
    totalBlocks: memory.total_blocks,
    freeBlocks: memory.free_blocks,
    trackedPids: memory.tracked_pids,
    allocatedTensors: memory.allocated_tensors,
    allocBytes: memory.alloc_bytes,
    evictions: memory.evictions,
    swapCount: memory.swap_count,
    swapFaults: memory.swap_faults,
    swapFailures: memory.swap_failures,
    pendingSwaps: memory.pending_swaps,
    parkedPids: memory.parked_pids,
    oomEvents: memory.oom_events,
    swapWorkerCrashes: memory.swap_worker_crashes,
  };
}

function mapRuntimeInstance(
  runtime: LobbySnapshotDto["runtime_instances"][number],
): RuntimeInstance {
  return {
    runtimeId: runtime.runtime_id,
    targetKind: runtime.target_kind,
    logicalModelId: runtime.logical_model_id,
    displayPath: runtime.display_path,
    family: runtime.family,
    backendId: runtime.backend_id,
    backendClass: runtime.backend_class,
    providerId: runtime.provider_id,
    remoteModelId: runtime.remote_model_id,
    state: runtime.state,
    reservationRamBytes: runtime.reservation_ram_bytes,
    reservationVramBytes: runtime.reservation_vram_bytes,
    pinned: runtime.pinned,
    transitionState: runtime.transition_state,
    activePidCount: runtime.active_pid_count,
    activePids: runtime.active_pids,
    current: runtime.current,
  };
}

function mapRuntimeLoadQueueEntry(
  entry: LobbySnapshotDto["runtime_load_queue"][number],
): RuntimeLoadQueueEntry {
  return {
    queueId: entry.queue_id,
    logicalModelId: entry.logical_model_id,
    displayPath: entry.display_path,
    backendClass: entry.backend_class,
    state: entry.state,
    reservationRamBytes: entry.reservation_ram_bytes,
    reservationVramBytes: entry.reservation_vram_bytes,
    reason: entry.reason,
    requestedAtMs: entry.requested_at_ms,
    updatedAtMs: entry.updated_at_ms,
  };
}

function mapResourceGovernor(
  governor: LobbySnapshotDto["resource_governor"],
): ResourceGovernorStatus | null {
  if (!governor) {
    return null;
  }

  return {
    ramBudgetBytes: governor.ram_budget_bytes,
    vramBudgetBytes: governor.vram_budget_bytes,
    minRamHeadroomBytes: governor.min_ram_headroom_bytes,
    minVramHeadroomBytes: governor.min_vram_headroom_bytes,
    ramUsedBytes: governor.ram_used_bytes,
    vramUsedBytes: governor.vram_used_bytes,
    ramAvailableBytes: governor.ram_available_bytes,
    vramAvailableBytes: governor.vram_available_bytes,
    pendingQueueDepth: governor.pending_queue_depth,
    loaderBusy: governor.loader_busy,
    loaderReason: governor.loader_reason,
  };
}

export async function fetchLobbySnapshot(): Promise<LobbySnapshot> {
  const snapshot = await invoke<LobbySnapshotDto>("fetch_lobby_snapshot");
  return normalizeLobbySnapshot(snapshot);
}

export async function fetchWorkspaceSnapshot(
  sessionId: string,
  pid: number | null,
): Promise<WorkspaceSnapshot> {
  const snapshot = await invoke<WorkspaceSnapshotDto>("fetch_workspace_snapshot", {
    sessionId,
    pid,
  });
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

export async function resumeSession(
  sessionId: string,
): Promise<StartSessionResult> {
  const session = await invoke<{
    session_id: string;
    pid: number;
  }>("resume_session", { sessionId });

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

export async function scheduleWorkflowJob(
  payload: string,
): Promise<ScheduleJobResult> {
  const result = await invoke<{
    job_id: number;
    next_run_at_ms: number | null;
    trigger_kind: string;
  }>("schedule_workflow_job", { payload });

  return {
    jobId: result.job_id,
    nextRunAtMs: result.next_run_at_ms,
    triggerKind: result.trigger_kind,
  };
}

export async function listOrchestrations(): Promise<LobbyOrchestrationSummary[]> {
  const result = await invoke<{
    orchestrations: Array<{
      orchestration_id: number;
      total: number;
      completed: number;
      running: number;
      pending: number;
      failed: number;
      skipped: number;
      finished: boolean;
      elapsed_secs: number;
      policy: string;
    }>;
  }>("list_orchestrations");

  return result.orchestrations.map((orchestration) => ({
    orchestrationId: orchestration.orchestration_id,
    total: orchestration.total,
    completed: orchestration.completed,
    running: orchestration.running,
    pending: orchestration.pending,
    failed: orchestration.failed,
    skipped: orchestration.skipped,
    finished: orchestration.finished,
    elapsedLabel: formatElapsedLabel(orchestration.elapsed_secs),
    policy: orchestration.policy,
  }));
}

export async function listScheduledJobs(): Promise<ScheduledJob[]> {
  const result = await invoke<{
    jobs: Array<{
      job_id: number;
      name: string;
      target_kind: string;
      trigger_kind: string;
      trigger_label: string;
      enabled: boolean;
      state: string;
      next_run_at_ms: number | null;
      current_trigger_at_ms: number | null;
      current_attempt: number;
      timeout_ms: number;
      max_retries: number;
      backoff_ms: number;
      last_run_started_at_ms: number | null;
      last_run_completed_at_ms: number | null;
      last_run_status: string | null;
      last_error: string | null;
      consecutive_failures: number;
      active_orchestration_id: number | null;
      recent_runs: Array<{
        run_id: number;
        trigger_at_ms: number;
        attempt: number;
        status: string;
        started_at_ms: number | null;
        completed_at_ms: number | null;
        orchestration_id: number | null;
        deadline_at_ms: number | null;
        error: string | null;
      }>;
    }>;
  }>("list_scheduled_jobs");

  return result.jobs.map((job) => ({
    jobId: job.job_id,
    name: job.name,
    targetKind: job.target_kind,
    triggerKind: job.trigger_kind,
    triggerLabel: job.trigger_label,
    enabled: job.enabled,
    state: job.state,
    nextRunAtMs: job.next_run_at_ms,
    currentTriggerAtMs: job.current_trigger_at_ms,
    currentAttempt: job.current_attempt,
    timeoutMs: job.timeout_ms,
    maxRetries: job.max_retries,
    backoffMs: job.backoff_ms,
    lastRunStartedAtMs: job.last_run_started_at_ms,
    lastRunCompletedAtMs: job.last_run_completed_at_ms,
    lastRunStatus: job.last_run_status,
    lastError: job.last_error,
    consecutiveFailures: job.consecutive_failures,
    activeOrchestrationId: job.active_orchestration_id,
    recentRuns: job.recent_runs.map((run) => ({
      runId: run.run_id,
      triggerAtMs: run.trigger_at_ms,
      attempt: run.attempt,
      status: run.status,
      startedAtMs: run.started_at_ms,
      completedAtMs: run.completed_at_ms,
      orchestrationId: run.orchestration_id,
      deadlineAtMs: run.deadline_at_ms,
      error: run.error,
    })),
  }));
}

export async function listWorkflowArtifacts(
  orchestrationId: number,
  task: string | null = null,
): Promise<OrchestrationArtifact[]> {
  const result = await invoke<{
    orchestration_id: number;
    task: string | null;
    artifacts: Array<{
      artifact_id: string;
      task: string;
      attempt: number;
      kind: string;
      label: string;
      mime_type: string;
      preview: string;
      content: string;
      bytes: number;
      created_at_ms: number;
    }>;
  }>("list_workflow_artifacts", {
    orchestrationId,
    task,
  });

  return result.artifacts.map((artifact) => ({
    artifactId: artifact.artifact_id,
    task: artifact.task,
    attempt: artifact.attempt,
    kind: artifact.kind,
    label: artifact.label,
    mimeType: artifact.mime_type,
    preview: artifact.preview,
    content: artifact.content,
    bytes: artifact.bytes,
    createdAtMs: artifact.created_at_ms,
  }));
}

export async function fetchOrchestrationStatus(
  orchestrationId: number,
): Promise<OrchestrationStatus> {
  const result = await invoke<{
    orchestration_id: number;
    total: number;
    completed: number;
    running: number;
    pending: number;
    failed: number;
    skipped: number;
    finished: boolean;
    elapsed_secs: number;
    policy: string;
    truncations: number;
    output_chars_stored: number;
    tasks: Array<{
      task: string;
      role?: string | null;
      workload?: string | null;
      backend_class?: string | null;
      context_strategy?: string | null;
      deps?: string[];
      status: string;
      current_attempt?: number | null;
      pid: number | null;
      error: string | null;
      context: null | {
        context_strategy: string;
        context_tokens_used: number;
        context_window_size: number;
        context_compressions: number;
        context_retrieval_hits: number;
        context_retrieval_requests: number;
        context_retrieval_misses: number;
        context_retrieval_candidates_scored: number;
        context_retrieval_segments_selected: number;
        last_retrieval_candidates_scored: number;
        last_retrieval_segments_selected: number;
        last_retrieval_latency_ms: number;
        last_retrieval_top_score?: number | null;
        last_compaction_reason: string | null;
        last_summary_ts: string | null;
        context_segments: number;
        episodic_segments: number;
        episodic_tokens: number;
        retrieve_top_k: number;
        retrieve_candidate_limit: number;
        retrieve_max_segment_chars: number;
        retrieve_min_score: number;
      };
      latest_output_preview?: string | null;
      latest_output_text?: string | null;
      latest_output_truncated?: boolean;
      input_artifacts?: Array<{
        artifact_id: string;
        task: string;
        attempt: number;
        kind: string;
        label: string;
      }>;
      output_artifacts?: Array<{
        artifact_id: string;
        task: string;
        attempt: number;
        kind: string;
        label: string;
        mime_type: string;
        preview: string;
        content: string;
        bytes: number;
        created_at_ms: number;
      }>;
      attempts?: Array<{
        attempt: number;
        status: string;
        session_id?: string | null;
        pid?: number | null;
        error?: string | null;
        output_preview: string;
        output_chars: number;
        truncated: boolean;
        started_at_ms: number;
        completed_at_ms?: number | null;
        primary_artifact_id?: string | null;
      }>;
    }>;
  }>("fetch_orchestration_status", { orchestrationId });

  return {
    orchestrationId: result.orchestration_id,
    total: result.total,
    completed: result.completed,
    running: result.running,
    pending: result.pending,
    failed: result.failed,
    skipped: result.skipped,
    finished: result.finished,
    elapsedSecs: result.elapsed_secs,
    policy: result.policy,
    truncations: result.truncations,
    outputCharsStored: result.output_chars_stored,
    tasks: result.tasks.map((task) => ({
      task: task.task,
      role: task.role ?? null,
      workload: task.workload ?? null,
      backendClass: task.backend_class ?? null,
      contextStrategy: task.context_strategy ?? null,
      deps: task.deps ?? [],
      status: task.status,
      currentAttempt: task.current_attempt ?? null,
      pid: task.pid,
      error: task.error,
      context: task.context
        ? {
            contextStrategy: task.context.context_strategy,
            contextTokensUsed: task.context.context_tokens_used,
            contextWindowSize: task.context.context_window_size,
            contextCompressions: task.context.context_compressions,
            contextRetrievalHits: task.context.context_retrieval_hits,
            contextRetrievalRequests:
              task.context.context_retrieval_requests,
            contextRetrievalMisses: task.context.context_retrieval_misses,
            contextRetrievalCandidatesScored:
              task.context.context_retrieval_candidates_scored,
            contextRetrievalSegmentsSelected:
              task.context.context_retrieval_segments_selected,
            lastRetrievalCandidatesScored:
              task.context.last_retrieval_candidates_scored,
            lastRetrievalSegmentsSelected:
              task.context.last_retrieval_segments_selected,
            lastRetrievalLatencyMs:
              task.context.last_retrieval_latency_ms,
            lastRetrievalTopScore:
              task.context.last_retrieval_top_score ?? null,
            lastCompactionReason: task.context.last_compaction_reason,
            lastSummaryTs: task.context.last_summary_ts,
            contextSegments: task.context.context_segments,
            episodicSegments: task.context.episodic_segments,
            episodicTokens: task.context.episodic_tokens,
            retrieveTopK: task.context.retrieve_top_k,
            retrieveCandidateLimit:
              task.context.retrieve_candidate_limit,
            retrieveMaxSegmentChars:
              task.context.retrieve_max_segment_chars,
            retrieveMinScore: task.context.retrieve_min_score,
          }
        : null,
      latestOutputPreview: task.latest_output_preview ?? null,
      latestOutputText: task.latest_output_text ?? null,
      latestOutputTruncated: task.latest_output_truncated ?? false,
      inputArtifacts: (task.input_artifacts ?? []).map((artifact) => ({
        artifactId: artifact.artifact_id,
        task: artifact.task,
        attempt: artifact.attempt,
        kind: artifact.kind,
        label: artifact.label,
      })),
      outputArtifacts: (task.output_artifacts ?? []).map((artifact) => ({
        artifactId: artifact.artifact_id,
        task: artifact.task,
        attempt: artifact.attempt,
        kind: artifact.kind,
        label: artifact.label,
        mimeType: artifact.mime_type,
        preview: artifact.preview,
        content: artifact.content,
        bytes: artifact.bytes,
        createdAtMs: artifact.created_at_ms,
      })),
      attempts: (task.attempts ?? []).map((attempt) => ({
        attempt: attempt.attempt,
        status: attempt.status,
        sessionId: attempt.session_id ?? null,
        pid: attempt.pid ?? null,
        error: attempt.error ?? null,
        outputPreview: attempt.output_preview,
        outputChars: attempt.output_chars,
        truncated: attempt.truncated,
        startedAtMs: attempt.started_at_ms,
        completedAtMs: attempt.completed_at_ms ?? null,
        primaryArtifactId: attempt.primary_artifact_id ?? null,
      })),
    })),
  };
}

export async function retryWorkflowTask(
  orchestrationId: number,
  taskId: string,
): Promise<RetryWorkflowTaskResult> {
  const result = await invoke<{
    orchestration_id: number;
    task: string;
    reset_tasks: string[];
    spawned: number;
  }>("retry_workflow_task", { orchestrationId, taskId });

  return {
    orchestrationId: result.orchestration_id,
    task: result.task,
    resetTasks: result.reset_tasks,
    spawned: result.spawned,
  };
}

export async function stopWorkflowRun(
  orchestrationId: number,
): Promise<WorkflowRunControlResult> {
  const result = await invoke<{
    orchestration_id: number;
    status: string;
  }>("stop_workflow_run", { orchestrationId });

  return {
    orchestrationId: result.orchestration_id,
    status: result.status,
  };
}

export async function deleteWorkflowRun(
  orchestrationId: number,
): Promise<WorkflowRunControlResult> {
  const result = await invoke<{
    orchestration_id: number;
    status: string;
  }>("delete_workflow_run", { orchestrationId });

  return {
    orchestrationId: result.orchestration_id,
    status: result.status,
  };
}

export async function setScheduledJobEnabled(
  jobId: number,
  enabled: boolean,
): Promise<ScheduledJobControlResult> {
  const result = await invoke<{
    job_id: number;
    enabled: boolean;
    state: string;
  }>("set_scheduled_job_enabled", { jobId, enabled });

  return {
    jobId: result.job_id,
    enabled: result.enabled,
    state: result.state,
  };
}

export async function deleteScheduledJob(
  jobId: number,
): Promise<ScheduledJobControlResult> {
  const result = await invoke<{
    job_id: number;
    enabled: boolean;
    state: string;
  }>("delete_scheduled_job", { jobId });

  return {
    jobId: result.job_id,
    enabled: result.enabled,
    state: result.state,
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
      resolved_backend_class: string | null;
      resolved_backend_capabilities: {
        resident_kv: boolean;
        persistent_slots: boolean;
        save_restore_slots: boolean;
        prompt_cache_reuse: boolean;
        streaming_generation: boolean;
        structured_output: boolean;
        cancel_generation: boolean;
        memory_telemetry: boolean;
        tool_pause_resume: boolean;
        context_compaction_reset: boolean;
        parallel_sessions: boolean;
      } | null;
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
      resolved_backend_class: string | null;
      resolved_backend_capabilities: {
        resident_kv: boolean;
        persistent_slots: boolean;
        save_restore_slots: boolean;
        prompt_cache_reuse: boolean;
        streaming_generation: boolean;
        structured_output: boolean;
        cancel_generation: boolean;
        memory_telemetry: boolean;
        tool_pause_resume: boolean;
        context_compaction_reset: boolean;
        parallel_sessions: boolean;
      } | null;
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
    remote_providers: Array<{
      id: string;
      backend_id: string;
      adapter_kind: string;
      label: string;
      note: string | null;
      credential_hint: string | null;
      default_model_id: string;
      models: Array<{
        id: string;
        label: string;
        context_window_tokens: number | null;
        max_output_tokens: number | null;
        supports_structured_output: boolean;
        input_price_usd_per_mtok: number | null;
        output_price_usd_per_mtok: number | null;
      }>;
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
      resolvedBackendClass: model.resolved_backend_class,
      resolvedBackendCapabilities: mapBackendCapabilities(
        model.resolved_backend_capabilities,
      ),
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
      resolvedBackendClass: entry.resolved_backend_class,
      resolvedBackendCapabilities: mapBackendCapabilities(
        entry.resolved_backend_capabilities,
      ),
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
    remoteProviders: snapshot.remote_providers.map((provider) => ({
      id: provider.id,
      backendId: provider.backend_id,
      adapterKind: provider.adapter_kind,
      label: provider.label,
      note: provider.note,
      credentialHint: provider.credential_hint,
      defaultModelId: provider.default_model_id,
      models: provider.models.map((model) => ({
        id: model.id,
        label: model.label,
        contextWindowTokens: model.context_window_tokens,
        maxOutputTokens: model.max_output_tokens,
        supportsStructuredOutput: model.supports_structured_output,
        inputPriceUsdPerMtok: model.input_price_usd_per_mtok,
        outputPriceUsdPerMtok: model.output_price_usd_per_mtok,
      })),
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
    loaded_model_id: string;
    loaded_target_kind: string;
    loaded_provider_id: string | null;
    loaded_remote_model_id: string | null;
    backend: string;
    backend_class: string;
    backend_capabilities: {
      resident_kv: boolean;
      persistent_slots: boolean;
      save_restore_slots: boolean;
      prompt_cache_reuse: boolean;
      streaming_generation: boolean;
      structured_output: boolean;
      cancel_generation: boolean;
      memory_telemetry: boolean;
      tool_pause_resume: boolean;
      context_compaction_reset: boolean;
      parallel_sessions: boolean;
    };
    driver_source: string;
    driver_rationale: string;
    path: string;
    architecture: string | null;
    load_mode: string;
    remote_model: LobbySnapshotDto["loaded_remote_model"];
  }>("load_model", { selector });

  return {
    family: result.family,
    loadedModelId: result.loaded_model_id,
    loadedTargetKind: result.loaded_target_kind,
    loadedProviderId: result.loaded_provider_id,
    loadedRemoteModelId: result.loaded_remote_model_id,
    backend: result.backend,
    backendClass: result.backend_class,
    backendCapabilities: mapBackendCapabilities(result.backend_capabilities)!,
    driverSource: result.driver_source,
    driverRationale: result.driver_rationale,
    path: result.path,
    architecture: result.architecture,
    loadMode: result.load_mode,
    remoteModel: mapRemoteRuntimeModel(result.remote_model),
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

export async function fetchTimelineSnapshot(
  sessionId: string,
  pid: number | null,
): Promise<TimelineSnapshot> {
  const snapshot = await invoke<TimelineSnapshotDto>("fetch_timeline_snapshot", {
    sessionId,
    pid,
  });
  return normalizeTimelineSnapshot(snapshot);
}

export async function deleteSession(sessionId: string): Promise<void> {
  await invoke("delete_session", { sessionId });
}
