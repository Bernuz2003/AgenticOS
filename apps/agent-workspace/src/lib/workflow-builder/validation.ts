import type {
  DraftBackendClass,
  DraftContextStrategy,
  DraftTask,
  DraftWorkload,
} from "./index";

export const workloadOptions: Array<{ value: DraftWorkload; label: string }> = [
  { value: "", label: "Default routing" },
  { value: "general", label: "General" },
  { value: "fast", label: "Fast" },
  { value: "code", label: "Code" },
  { value: "reasoning", label: "Reasoning" },
];

export const backendOptions: Array<{ value: DraftBackendClass; label: string }> = [
  { value: "", label: "Auto target" },
  { value: "resident_local", label: "Resident local" },
  { value: "remote_stateless", label: "Remote stateless" },
];

export const contextOptions: Array<{ value: DraftContextStrategy; label: string }> = [
  { value: "", label: "Kernel default" },
  { value: "sliding_window", label: "Sliding window" },
  { value: "summarize", label: "Summarize" },
  { value: "retrieve", label: "Retrieve" },
];

export function splitDeps(value: string): string[] {
  return value
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

export function validateDraftTask(
  task: DraftTask,
  index: number,
  seen: Set<string>,
) {
  const id = task.id.trim();
  const prompt = task.prompt.trim();
  if (!id) {
    throw new Error(`Task ${index + 1} is missing an id.`);
  }
  if (!prompt) {
    throw new Error(`Task ${id} is missing a prompt.`);
  }
  if (seen.has(id)) {
    throw new Error(`Duplicate task id '${id}'.`);
  }
  seen.add(id);

  const deps = splitDeps(task.depsText);
  if (deps.includes(id)) {
    throw new Error(`Task ${id} cannot depend on itself.`);
  }

  return { id, prompt, deps };
}

export function validateScheduledJobName(name: string): string {
  const normalized = name.trim();
  if (!normalized) {
    throw new Error("Scheduled jobs require a name.");
  }
  return normalized;
}
