import type { RemoteProvider, RemoteProviderModel } from "./api";

export interface RemoteCatalogSelection {
  provider: RemoteProvider | null;
  model: RemoteProviderModel | null;
  selector: string;
}

export function findRemoteProvider(
  providers: RemoteProvider[],
  providerId: string | null | undefined,
): RemoteProvider | null {
  if (!providerId) {
    return providers[0] ?? null;
  }

  return providers.find((provider) => provider.id === providerId) ?? providers[0] ?? null;
}

export function findRemoteModel(
  provider: RemoteProvider | null,
  modelId: string | null | undefined,
): RemoteProviderModel | null {
  if (!provider) {
    return null;
  }

  if (modelId) {
    const match = provider.models.find((model) => model.id === modelId);
    if (match) {
      return match;
    }
  }

  return (
    provider.models.find((model) => model.id === provider.defaultModelId) ??
    provider.models[0] ??
    null
  );
}

export function selectRemoteCatalogTarget(
  providers: RemoteProvider[],
  providerId: string | null | undefined,
  modelId: string | null | undefined,
): RemoteCatalogSelection {
  const provider = findRemoteProvider(providers, providerId);
  const model = findRemoteModel(provider, modelId);
  const selector =
    provider === null
      ? ""
      : model !== null
        ? `cloud:${provider.id}:${model.id}`
        : `cloud:${provider.id}`;

  return { provider, model, selector };
}

export function isLoadedLocalCatalogModel(
  loadedTargetKind: string | null | undefined,
  loadedModelId: string,
  modelId: string,
): boolean {
  return loadedTargetKind === "local_catalog" && loadedModelId === modelId;
}

export function matchesLoadedRemoteTarget(
  loadedTargetKind: string | null | undefined,
  loadedProviderId: string | null | undefined,
  loadedRemoteModelId: string | null | undefined,
  providerId: string | null | undefined,
  modelId: string | null | undefined,
): boolean {
  return (
    loadedTargetKind === "remote_provider" &&
    !!providerId &&
    !!modelId &&
    loadedProviderId === providerId &&
    loadedRemoteModelId === modelId
  );
}
