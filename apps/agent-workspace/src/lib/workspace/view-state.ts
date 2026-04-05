export type WorkspaceMode = "conversation" | "debug";

export function workspaceModeFromSearch(
  modeParam: string | null | undefined,
  _dumpParam: string | null | undefined,
): WorkspaceMode {
  if (modeParam === "debug") {
    return "debug";
  }
  return "conversation";
}

export function updateWorkspaceSearchParams(
  current: URLSearchParams,
  patch: {
    mode?: WorkspaceMode | null;
    dump?: string | null;
  },
): URLSearchParams {
  const next = new URLSearchParams(current);

  if ("mode" in patch) {
    if (!patch.mode || patch.mode === "conversation") {
      next.delete("mode");
      if (!("dump" in patch)) {
        next.delete("dump");
      }
    } else {
      next.set("mode", patch.mode);
    }
  }

  if ("dump" in patch) {
    if (!patch.dump) {
      next.delete("dump");
    } else {
      next.set("dump", patch.dump);
    }
  }

  return next;
}
