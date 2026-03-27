import { splitDeps, type DraftTask } from "./index";

export interface GraphNodePosition {
  x: number;
  y: number;
}

export interface WorkflowDraftValidation {
  errors: string[];
  warnings: string[];
}

export function taskDisplayId(task: DraftTask, index: number): string {
  return task.id.trim() || `task_${index + 1}`;
}

function buildIdIndexMap(tasks: DraftTask[]): Map<string, number> {
  const map = new Map<string, number>();
  tasks.forEach((task, index) => {
    const id = task.id.trim();
    if (id && !map.has(id)) {
      map.set(id, index);
    }
  });
  return map;
}

function taskDeps(tasks: DraftTask[], index: number): string[] {
  return splitDeps(tasks[index]?.depsText ?? "");
}

function taskDependentsCount(tasks: DraftTask[], taskId: string): number {
  return tasks.filter((task) => splitDeps(task.depsText).includes(taskId)).length;
}

function hasPath(tasks: DraftTask[], startId: string, targetId: string): boolean {
  const idIndexMap = buildIdIndexMap(tasks);
  const stack = [startId];
  const seen = new Set<string>();

  while (stack.length > 0) {
    const currentId = stack.pop() ?? "";
    if (!currentId || seen.has(currentId)) {
      continue;
    }
    seen.add(currentId);
    const currentIndex = idIndexMap.get(currentId);
    if (currentIndex === undefined) {
      continue;
    }
    const dependents = tasks
      .filter((task) => splitDeps(task.depsText).includes(currentId))
      .map((task) => task.id.trim())
      .filter(Boolean);
    if (dependents.includes(targetId)) {
      return true;
    }
    stack.push(...dependents);
  }

  return false;
}

function hasCycle(tasks: DraftTask[]): boolean {
  const idIndexMap = buildIdIndexMap(tasks);
  const visiting = new Set<string>();
  const visited = new Set<string>();

  function visit(taskId: string): boolean {
    if (visiting.has(taskId)) {
      return true;
    }
    if (visited.has(taskId)) {
      return false;
    }
    visiting.add(taskId);
    const taskIndex = idIndexMap.get(taskId);
    if (taskIndex !== undefined) {
      for (const depId of taskDeps(tasks, taskIndex)) {
        if (idIndexMap.has(depId) && visit(depId)) {
          return true;
        }
      }
    }
    visiting.delete(taskId);
    visited.add(taskId);
    return false;
  }

  return [...idIndexMap.keys()].some((taskId) => visit(taskId));
}

function computeDepthForIndex(
  tasks: DraftTask[],
  index: number,
  memo: Map<number, number>,
  visiting: Set<number>,
): number {
  if (memo.has(index)) {
    return memo.get(index) ?? 0;
  }
  if (visiting.has(index)) {
    return 0;
  }
  visiting.add(index);
  const idIndexMap = buildIdIndexMap(tasks);
  const deps = taskDeps(tasks, index);
  const depth =
    deps.length === 0
      ? 0
      : Math.max(
          ...deps.map((depId) => {
            const depIndex = idIndexMap.get(depId);
            return depIndex === undefined
              ? 0
              : computeDepthForIndex(tasks, depIndex, memo, visiting) + 1;
          }),
        );
  visiting.delete(index);
  memo.set(index, depth);
  return depth;
}

export function buildInitialNodePositions(tasks: DraftTask[]): GraphNodePosition[] {
  const memo = new Map<number, number>();
  const columns = new Map<number, number[]>();

  tasks.forEach((_, index) => {
    const depth = computeDepthForIndex(tasks, index, memo, new Set<number>());
    const bucket = columns.get(depth) ?? [];
    bucket.push(index);
    columns.set(depth, bucket);
  });

  return tasks.map((_, index) => {
    const depth = memo.get(index) ?? 0;
    const row = (columns.get(depth) ?? []).indexOf(index);
    return {
      x: 48 + depth * 300,
      y: 48 + row * 172,
    };
  });
}

export function ensureNodePositions(
  tasks: DraftTask[],
  positions: GraphNodePosition[],
): GraphNodePosition[] {
  const defaults = buildInitialNodePositions(tasks);
  return tasks.map((_, index) => positions[index] ?? defaults[index]);
}

export function addDependency(
  tasks: DraftTask[],
  sourceIndex: number,
  targetIndex: number,
): DraftTask[] {
  const sourceId = tasks[sourceIndex]?.id.trim();
  if (!sourceId) {
    return tasks;
  }
  return tasks.map((task, index) => {
    if (index !== targetIndex) {
      return task;
    }
    const deps = splitDeps(task.depsText);
    if (!deps.includes(sourceId)) {
      deps.push(sourceId);
    }
    return {
      ...task,
      depsText: deps.join(", "),
    };
  });
}

export function removeDependency(
  tasks: DraftTask[],
  targetIndex: number,
  depId: string,
): DraftTask[] {
  return tasks.map((task, index) => {
    if (index !== targetIndex) {
      return task;
    }
    return {
      ...task,
      depsText: splitDeps(task.depsText)
        .filter((candidate) => candidate !== depId)
        .join(", "),
    };
  });
}

export function canAddDependency(
  tasks: DraftTask[],
  sourceIndex: number,
  targetIndex: number,
): boolean {
  if (sourceIndex === targetIndex) {
    return false;
  }
  const sourceId = tasks[sourceIndex]?.id.trim();
  const targetId = tasks[targetIndex]?.id.trim();
  if (!sourceId || !targetId || sourceId === targetId) {
    return false;
  }
  const deps = taskDeps(tasks, targetIndex);
  if (deps.includes(sourceId)) {
    return false;
  }
  return !hasPath(tasks, targetId, sourceId);
}

export function validateWorkflowDraft(tasks: DraftTask[]): WorkflowDraftValidation {
  const errors: string[] = [];
  const warnings: string[] = [];
  const idIndexMap = buildIdIndexMap(tasks);
  const seen = new Set<string>();

  tasks.forEach((task, index) => {
    const id = task.id.trim();
    if (!id) {
      errors.push(`Task ${index + 1} is missing an id.`);
    } else if (seen.has(id)) {
      errors.push(`Duplicate task id '${id}'.`);
    } else {
      seen.add(id);
    }

    if (!task.prompt.trim()) {
      errors.push(`Task ${taskDisplayId(task, index)} is missing a prompt.`);
    }

    const deps = splitDeps(task.depsText);
    deps.forEach((depId) => {
      if (depId === id) {
        errors.push(`Task ${id} cannot depend on itself.`);
      } else if (!idIndexMap.has(depId)) {
        errors.push(`Task ${taskDisplayId(task, index)} depends on missing task '${depId}'.`);
      }
    });

    if (id && deps.length === 0 && taskDependentsCount(tasks, id) === 0 && tasks.length > 1) {
      warnings.push(`Task ${id} is isolated from the rest of the DAG.`);
    }
  });

  if (hasCycle(tasks)) {
    errors.push("The workflow graph contains a cycle.");
  }

  return {
    errors: [...new Set(errors)],
    warnings: [...new Set(warnings)],
  };
}
