import { invoke } from "@tauri-apps/api/core";

import type {
  LoadModelResult,
  LobbySnapshotDto,
  ModelCatalogSnapshot,
  SelectModelResult,
} from "./index";
import {
  mapBackendCapabilities,
  mapRemoteRuntimeModel,
} from "./normalizers";

export async function listModels(): Promise<ModelCatalogSnapshot> {
  const snapshot = await invoke<any>("list_models");

  return {
    selectedModelId: snapshot.selected_model_id,
    totalModels: snapshot.total_models,
    models: snapshot.models.map((model: any) => ({
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
    routingRecommendations: snapshot.routing_recommendations.map((entry: any) => ({
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
    remoteProviders: snapshot.remote_providers.map((provider: any) => ({
      id: provider.id,
      backendId: provider.backend_id,
      adapterKind: provider.adapter_kind,
      label: provider.label,
      note: provider.note,
      credentialHint: provider.credential_hint,
      defaultModelId: provider.default_model_id,
      models: provider.models.map((model: any) => ({
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
    backend_capabilities: NonNullable<LobbySnapshotDto["loaded_backend_capabilities"]>;
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
