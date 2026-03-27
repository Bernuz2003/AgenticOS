import type {
  AuditEvent,
  AuditEventDto,
  BackendCapabilities,
  BackendTelemetry,
  HumanInputRequest,
  LobbySnapshot,
  LobbySnapshotDto,
  ManagedLocalRuntime,
  MemoryStatus,
  RemoteRuntimeModel,
  ResourceGovernorStatus,
  RuntimeInstance,
  RuntimeLoadQueueEntry,
  ScheduledJob,
  TimelineSnapshot,
  TimelineSnapshotDto,
  WorkspaceContextSnapshot,
  WorkspaceSnapshot,
  WorkspaceSnapshotDto,
} from "./index";

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
    managedLocalRuntimes: snapshot.managed_local_runtimes.map(
      mapManagedLocalRuntime,
    ),
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
    context: snapshot.context ? mapWorkspaceContext(snapshot.context) : null,
    pendingHumanRequest: snapshot.pending_human_request
      ? mapHumanInputRequest(snapshot.pending_human_request)
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

export function formatElapsedLabel(seconds: number): string {
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

export function mapBackendCapabilities(
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

export function mapRemoteRuntimeModel(
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

export function mapBackendTelemetry(
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

function mapMemoryStatus(memory: LobbySnapshotDto["memory"]): MemoryStatus | null {
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

function mapManagedLocalRuntime(
  runtime: LobbySnapshotDto["managed_local_runtimes"][number],
): ManagedLocalRuntime {
  return {
    family: runtime.family,
    logicalModelId: runtime.logical_model_id,
    displayPath: runtime.display_path,
    state: runtime.state,
    endpoint: runtime.endpoint,
    port: runtime.port,
    contextWindowTokens: runtime.context_window_tokens,
    slotSaveDir: runtime.slot_save_dir,
    managedByKernel: runtime.managed_by_kernel,
    lastError: runtime.last_error,
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

function mapWorkspaceContext(
  context: NonNullable<WorkspaceSnapshotDto["context"]>,
): WorkspaceContextSnapshot {
  return {
    contextStrategy: context.context_strategy,
    contextTokensUsed: context.context_tokens_used,
    contextWindowSize: context.context_window_size,
    contextCompressions: context.context_compressions,
    contextRetrievalHits: context.context_retrieval_hits,
    contextRetrievalRequests: context.context_retrieval_requests,
    contextRetrievalMisses: context.context_retrieval_misses,
    contextRetrievalCandidatesScored: context.context_retrieval_candidates_scored,
    contextRetrievalSegmentsSelected: context.context_retrieval_segments_selected,
    lastRetrievalCandidatesScored: context.last_retrieval_candidates_scored,
    lastRetrievalSegmentsSelected: context.last_retrieval_segments_selected,
    lastRetrievalLatencyMs: context.last_retrieval_latency_ms,
    lastRetrievalTopScore: context.last_retrieval_top_score,
    lastCompactionReason: context.last_compaction_reason,
    lastSummaryTs: context.last_summary_ts,
    contextSegments: context.context_segments,
    episodicSegments: context.episodic_segments,
    episodicTokens: context.episodic_tokens,
    retrieveTopK: context.retrieve_top_k,
    retrieveCandidateLimit: context.retrieve_candidate_limit,
    retrieveMaxSegmentChars: context.retrieve_max_segment_chars,
    retrieveMinScore: context.retrieve_min_score,
  };
}

function mapHumanInputRequest(
  request: NonNullable<WorkspaceSnapshotDto["pending_human_request"]>,
): HumanInputRequest {
  return {
    requestId: request.request_id,
    kind: request.kind,
    question: request.question,
    details: request.details,
    choices: request.choices,
    allowFreeText: request.allow_free_text,
    placeholder: request.placeholder,
    requestedAtMs: request.requested_at_ms,
  };
}
