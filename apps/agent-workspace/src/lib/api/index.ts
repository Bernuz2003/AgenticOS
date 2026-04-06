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

export interface McpDiscoveredTool {
  agenticToolName: string;
  targetName: string;
  title: string | null;
  description: string;
  dangerous: boolean;
  defaultAllowlisted: boolean;
  approvalRequired: boolean;
  readOnlyHint: boolean;
  destructiveHint: boolean;
  idempotentHint: boolean;
  openWorldHint: boolean;
}

export interface McpPrompt {
  name: string;
  title: string | null;
  description: string | null;
}

export interface McpResource {
  name: string;
  title: string | null;
  uri: string;
  description: string | null;
  mimeType: string | null;
}

export interface McpServerStatus {
  serverId: string;
  label: string | null;
  transport: string;
  trustLevel: string;
  authMode: string;
  health: string;
  toolPrefix: string;
  enabled: boolean;
  connected: boolean;
  defaultAllowlisted: boolean;
  approvalRequired: boolean;
  rootsEnabled: boolean;
  exposedTools: string[];
  discoveredTools: McpDiscoveredTool[];
  prompts: McpPrompt[];
  resources: McpResource[];
  lastLatencyMs: number | null;
  lastError: string | null;
}

export interface McpStatus {
  servers: McpServerStatus[];
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
  managedLocalRuntimes: ManagedLocalRuntime[];
  resourceGovernor: ResourceGovernorStatus | null;
  runtimeLoadQueue: RuntimeLoadQueueEntry[];
  mcp: McpStatus | null;
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
  requestId: string;
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
  lineage: WorkspaceLineage | null;
  replay: WorkspaceReplayDebugSnapshot | null;
}

export type WorkspaceBranchKind = "base" | "replay" | "fork";

export interface WorkspaceLineageBranch {
  sessionId: string;
  kind: WorkspaceBranchKind;
  title: string;
  createdAtMs: number;
  activePid: number | null;
  lastPid: number | null;
  sourceDumpId: string | null;
  selected: boolean;
}

export interface WorkspaceLineage {
  anchorSessionId: string;
  selectedSessionId: string;
  selectedKind: WorkspaceBranchKind;
  branches: WorkspaceLineageBranch[];
}

export interface WorkspaceReplayDebugSnapshot {
  sourceDumpId: string;
  sourceSessionId: string | null;
  sourcePid: number | null;
  sourceFidelity: string;
  replayMode: string;
  toolMode: string;
  initialState: string;
  patchedContextSegments: number;
  patchedEpisodicSegments: number;
  stubbedInvocations: number;
  overriddenInvocations: number;
  baseline: WorkspaceReplayBaselineSnapshot;
  diff: WorkspaceReplayDiffSnapshot;
}

export interface WorkspaceReplayBaselineSnapshot {
  sourceContextSegments: number;
  sourceEpisodicSegments: number;
  sourceReplayMessages: number;
  sourceToolInvocations: number;
  sourceContextChars: number;
  sourceEpisodicChars: number;
  sourceContextKinds: string[];
  sourceEpisodicKinds: string[];
}

export interface WorkspaceReplayDiffSnapshot {
  currentContextSegments: number | null;
  currentEpisodicSegments: number | null;
  currentReplayMessages: number;
  currentToolInvocations: number;
  contextChanged: boolean | null;
  contextSegmentsDelta: number | null;
  episodicSegmentsDelta: number | null;
  replayMessagesDelta: number;
  toolInvocationsDelta: number;
  branchOnlyMessages: number;
  branchOnlyToolCalls: number;
  changedToolOutputs: number;
  completedToolCalls: number;
  latestBranchMessage: string | null;
  invocationDiffs: WorkspaceReplayInvocationDiff[];
}

export interface WorkspaceReplayInvocationDiff {
  sourceToolCallId: string | null;
  replayToolCallId: string | null;
  toolName: string;
  commandText: string;
  sourceStatus: string | null;
  replayStatus: string | null;
  sourceOutputText: string | null;
  replayOutputText: string | null;
  branchOnly: boolean;
  changed: boolean;
}

export interface ProcessPermissions {
  trustScope: string;
  actionsAllowed: boolean;
  allowedTools: string[];
  pathGrants: PathGrant[];
  pathScopes: string[];
}

export type PathGrantAccessMode = "read_only" | "write_approved" | "autonomous_write";

export interface PathGrant {
  root: string;
  accessMode: PathGrantAccessMode;
  capsule: string | null;
  label: string | null;
  workspaceRelative: boolean;
}

export interface PathGrantInput {
  root: string;
  accessMode: PathGrantAccessMode;
  capsule?: string | null;
  label?: string | null;
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

export interface ManagedLocalRuntime {
  family: string;
  logicalModelId: string;
  displayPath: string;
  state: string;
  endpoint: string;
  port: number;
  contextWindowTokens: number | null;
  slotSaveDir: string;
  managedByKernel: boolean;
  lastError: string | null;
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

export interface CoreDumpSummary {
  dumpId: string;
  createdAtMs: number;
  sessionId: string | null;
  pid: number | null;
  reason: string;
  fidelity: string;
  path: string;
  bytes: number;
  sha256: string;
  note: string | null;
}

export interface CoreDumpManifestPreview {
  format: string | null;
  capture: {
    mode: string | null;
    reason: string | null;
    fidelity: string | null;
    note: string | null;
  };
  target: {
    source: string | null;
    state: string | null;
    sessionId: string | null;
    pid: number | null;
    runtimeId: string | null;
    inFlight: boolean | null;
  };
  session: null | {
    title: string | null;
    state: string | null;
  };
  process: null | {
    toolCaller: string | null;
    tokenCount: number | null;
    terminationReason: string | null;
    renderedPromptChars: number | null;
    promptChars: number | null;
  };
  counts: {
    replayMessages: number;
    debugCheckpoints: number;
    toolInvocations: number;
    toolAuditLines: number;
    sessionAuditEvents: number;
    workspaceEntries: number;
    limitations: number;
  };
  limitations: string[];
}

export interface CoreDumpInfo {
  dump: CoreDumpSummary;
  manifestJson: string;
  manifest: CoreDumpManifestPreview;
}

export interface ReplayCoreDumpResult {
  sourceDumpId: string;
  sessionId: string;
  pid: number;
  runtimeId: string;
  replaySessionTitle: string;
  replayFidelity: string;
  replayMode: string;
  toolMode: string;
  initialState: string;
  patchedContextSegments: number;
  patchedEpisodicSegments: number;
  stubbedInvocations: number;
  overriddenInvocations: number;
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
  terminationReason: string | null;
}

export interface OrchestrationIpcMessage {
  messageId: string;
  orchestrationId: number | null;
  senderPid: number | null;
  senderTask: string | null;
  senderAttempt: number | null;
  receiverPid: number | null;
  receiverTask: string | null;
  receiverAttempt: number | null;
  receiverRole: string | null;
  messageType: string;
  channel: string | null;
  payloadPreview: string;
  payloadText: string;
  status: string;
  createdAtMs: number;
  deliveredAtMs: number | null;
  consumedAtMs: number | null;
  failedAtMs: number | null;
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
  terminationReason: string | null;
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
  ipcMessages: OrchestrationIpcMessage[];
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
  managed_local_runtimes: Array<{
    family: string;
    logical_model_id: string;
    display_path: string;
    state: string;
    endpoint: string;
    port: number;
    context_window_tokens: number | null;
    slot_save_dir: string;
    managed_by_kernel: boolean;
    last_error: string | null;
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
  mcp: {
    servers: Array<{
      server_id: string;
      label: string | null;
      transport: string;
      trust_level: string;
      auth_mode: string;
      health: string;
      tool_prefix: string;
      enabled: boolean;
      connected: boolean;
      default_allowlisted: boolean;
      approval_required: boolean;
      roots_enabled: boolean;
      exposed_tools: string[];
      discovered_tools: Array<{
        agentic_tool_name: string;
        target_name: string;
        title: string | null;
        description: string;
        dangerous: boolean;
        default_allowlisted: boolean;
        approval_required: boolean;
        read_only_hint: boolean;
        destructive_hint: boolean;
        idempotent_hint: boolean;
        open_world_hint: boolean;
      }>;
      prompts: Array<{
        name: string;
        title: string | null;
        description: string | null;
      }>;
      resources: Array<{
        name: string;
        title: string | null;
        uri: string;
        description: string | null;
        mime_type: string | null;
      }>;
      last_latency_ms: number | null;
      last_error: string | null;
    }>;
  } | null;
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
    path_grants: Array<{
      root: string;
      access_mode: PathGrantAccessMode;
      capsule: string | null;
      label: string | null;
      workspace_relative: boolean;
    }>;
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
    request_id: string;
    kind: string;
    question: string;
    details: string | null;
    choices: string[];
    allow_free_text: boolean;
    placeholder: string | null;
    requested_at_ms: number;
  };
  audit_events: AuditEventDto[];
  lineage: null | {
    anchor_session_id: string;
    selected_session_id: string;
    selected_kind: WorkspaceBranchKind;
    branches: Array<{
      session_id: string;
      kind: WorkspaceBranchKind;
      title: string;
      created_at_ms: number;
      active_pid: number | null;
      last_pid: number | null;
      source_dump_id: string | null;
      selected: boolean;
    }>;
  };
  replay: null | {
    source_dump_id: string;
    source_session_id: string | null;
    source_pid: number | null;
    source_fidelity: string;
    replay_mode: string;
    tool_mode: string;
    initial_state: string;
    patched_context_segments: number;
    patched_episodic_segments: number;
    stubbed_invocations: number;
    overridden_invocations: number;
    baseline: {
      source_context_segments: number;
      source_episodic_segments: number;
      source_replay_messages: number;
      source_tool_invocations: number;
      source_context_chars: number;
      source_episodic_chars: number;
      source_context_kinds: string[];
      source_episodic_kinds: string[];
    };
    diff: {
      current_context_segments: number | null;
      current_episodic_segments: number | null;
      current_replay_messages: number;
      current_tool_invocations: number;
      context_changed: boolean | null;
      context_segments_delta: number | null;
      episodic_segments_delta: number | null;
      replay_messages_delta: number;
      tool_invocations_delta: number;
      branch_only_messages: number;
      branch_only_tool_calls: number;
      changed_tool_outputs: number;
      completed_tool_calls: number;
      latest_branch_message: string | null;
      invocation_diffs: Array<{
        source_tool_call_id: string | null;
        replay_tool_call_id: string | null;
        tool_name: string;
        command_text: string;
        source_status: string | null;
        replay_status: string | null;
        source_output_text: string | null;
        replay_output_text: string | null;
        branch_only: boolean;
        changed: boolean;
      }>;
    };
  };
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

export interface CoreDumpSummaryDto {
  dump_id: string;
  created_at_ms: number;
  session_id: string | null;
  pid: number | null;
  reason: string;
  fidelity: string;
  path: string;
  bytes: number;
  sha256: string;
  note: string | null;
}

export interface ReplayCoreDumpResultDto {
  source_dump_id: string;
  session_id: string;
  pid: number;
  runtime_id: string;
  replay_session_title: string;
  replay_fidelity: string;
  replay_mode: string;
  tool_mode: string;
  initial_state: string;
  patched_context_segments: number;
  patched_episodic_segments: number;
  stubbed_invocations: number;
  overridden_invocations: number;
}

export {
  auditEventKey,
  formatElapsedLabel,
  mapBackendCapabilities,
  mapBackendTelemetry,
  mapRemoteRuntimeModel,
  normalizeAuditEvent,
  normalizeLobbySnapshot,
  normalizeTimelineSnapshot,
  normalizeWorkspaceSnapshot,
} from "./normalizers";
export {
  fetchLobbySnapshot,
  fetchTimelineSnapshot,
  fetchWorkspaceSnapshot,
} from "./snapshots";
export { fetchCoreDumpInfo, listCoreDumps, captureCoreDump, replayCoreDump } from "./core-dumps";
export {
  continueSessionOutput,
  deleteSession,
  resumeSession,
  sendSessionInput,
  startSession,
  stopSessionOutput,
} from "./sessions";
export {
  deleteWorkflowRun,
  fetchOrchestrationStatus,
  listOrchestrations,
  listWorkflowArtifacts,
  orchestrate,
  retryWorkflowTask,
  stopWorkflowRun,
} from "./workflows";
export {
  deleteScheduledJob,
  listScheduledJobs,
  scheduleWorkflowJob,
  setScheduledJobEnabled,
} from "./jobs";
export { listModels, loadModel, selectModel } from "./models";
export { pingKernel, shutdownKernel } from "./runtime";
