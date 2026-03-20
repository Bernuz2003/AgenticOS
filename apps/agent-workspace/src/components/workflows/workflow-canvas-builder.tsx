import { useEffect, useMemo, useRef, useState } from "react";
import { Link2, Plus, Trash2 } from "lucide-react";
import type { DraftTask } from "../../lib/workflow-builder";
import {
  addDependency,
  buildInitialNodePositions,
  canAddDependency,
  ensureNodePositions,
  removeDependency,
  taskDisplayId,
} from "../../lib/workflow-graph";

const NODE_WIDTH = 248;
const NODE_HEIGHT = 138;

interface DragState {
  index: number;
  offsetX: number;
  offsetY: number;
}

interface LinkState {
  sourceIndex: number;
  pointerX: number;
  pointerY: number;
}

interface WorkflowCanvasBuilderProps {
  tasks: DraftTask[];
  selectedTaskIndex: number | null;
  onSelectTask: (index: number) => void;
  onTasksChange: (tasks: DraftTask[]) => void;
  onAddTask: () => void;
  onRemoveTask: (index: number) => void;
}

function edgePath(
  startX: number,
  startY: number,
  endX: number,
  endY: number,
): string {
  const delta = Math.max(72, Math.abs(endX - startX) * 0.45);
  return `M ${startX} ${startY} C ${startX + delta} ${startY}, ${endX - delta} ${endY}, ${endX} ${endY}`;
}

function workloadTone(workload: DraftTask["workload"]): string {
  switch (workload) {
    case "reasoning":
      return "border-indigo-200 bg-indigo-50 text-indigo-700";
    case "code":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "fast":
      return "border-amber-200 bg-amber-50 text-amber-700";
    default:
      return "border-slate-200 bg-slate-100 text-slate-600";
  }
}

export function WorkflowCanvasBuilder({
  tasks,
  selectedTaskIndex,
  onSelectTask,
  onTasksChange,
  onAddTask,
  onRemoveTask,
}: WorkflowCanvasBuilderProps) {
  const innerRef = useRef<HTMLDivElement | null>(null);
  const [positions, setPositions] = useState(() => buildInitialNodePositions(tasks));
  const [dragState, setDragState] = useState<DragState | null>(null);
  const [linkState, setLinkState] = useState<LinkState | null>(null);

  useEffect(() => {
    setPositions((current) => ensureNodePositions(tasks, current));
  }, [tasks]);

  const canvasSize = useMemo(() => {
    const maxX = positions.reduce((largest, position) => Math.max(largest, position.x), 0);
    const maxY = positions.reduce((largest, position) => Math.max(largest, position.y), 0);
    return {
      width: Math.max(1040, maxX + NODE_WIDTH + 96),
      height: Math.max(620, maxY + NODE_HEIGHT + 96),
    };
  }, [positions]);

  function relativePointer(clientX: number, clientY: number): { x: number; y: number } {
    const rect = innerRef.current?.getBoundingClientRect();
    if (!rect) {
      return { x: 0, y: 0 };
    }
    return {
      x: Math.max(24, clientX - rect.left),
      y: Math.max(24, clientY - rect.top),
    };
  }

  useEffect(() => {
    if (!dragState && !linkState) {
      return;
    }

    function handlePointerMove(event: PointerEvent) {
      if (dragState) {
        const { x, y } = relativePointer(event.clientX, event.clientY);
        setPositions((current) =>
          current.map((position, index) =>
            index === dragState.index
              ? {
                  x: Math.max(24, x - dragState.offsetX),
                  y: Math.max(24, y - dragState.offsetY),
                }
              : position,
          ),
        );
      }
      if (linkState) {
        const { x, y } = relativePointer(event.clientX, event.clientY);
        setLinkState((current) =>
          current
            ? {
                ...current,
                pointerX: x,
                pointerY: y,
              }
            : null,
        );
      }
    }

    function handlePointerUp(event: PointerEvent) {
      if (linkState) {
        const target = document
          .elementFromPoint(event.clientX, event.clientY)
          ?.closest<HTMLElement>("[data-workflow-node-index]");
        const targetIndex = target
          ? Number.parseInt(target.dataset.workflowNodeIndex ?? "", 10)
          : NaN;
        if (
          Number.isFinite(targetIndex) &&
          canAddDependency(tasks, linkState.sourceIndex, targetIndex)
        ) {
          onTasksChange(addDependency(tasks, linkState.sourceIndex, targetIndex));
          onSelectTask(targetIndex);
        }
      }

      setDragState(null);
      setLinkState(null);
    }

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp, { once: false });
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }, [dragState, linkState, onSelectTask, onTasksChange, tasks]);

  return (
    <div className="rounded-3xl border border-slate-200 bg-slate-50 p-4">
      <div className="mb-4 flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
        <div>
          <div className="text-xs font-bold uppercase tracking-[0.18em] text-slate-400">
            Visual DAG Builder
          </div>
          <h3 className="mt-1 text-lg font-bold text-slate-900">
            Drag nodes to arrange the graph. Drag a link handle to create dependencies.
          </h3>
        </div>
        <div className="flex flex-wrap gap-3">
          {linkState && (
            <div className="rounded-xl border border-indigo-200 bg-indigo-50 px-3 py-2 text-xs font-semibold text-indigo-700">
              Linking from {taskDisplayId(tasks[linkState.sourceIndex], linkState.sourceIndex)}
            </div>
          )}
          <button
            type="button"
            onClick={onAddTask}
            className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
          >
            <Plus className="h-4 w-4" />
            Add task
          </button>
        </div>
      </div>

      <div className="overflow-auto rounded-[28px] border border-slate-200 bg-white">
        <div
          ref={innerRef}
          className="relative"
          style={{ width: canvasSize.width, height: canvasSize.height }}
        >
          <svg className="pointer-events-none absolute inset-0 h-full w-full">
            {tasks.map((task, targetIndex) =>
              task.depsText
                .split(/[,\n]/)
                .map((depId) => depId.trim())
                .filter(Boolean)
                .map((depId) => {
                  const sourceIndex = tasks.findIndex(
                    (candidate) => candidate.id.trim() === depId,
                  );
                  if (sourceIndex < 0 || !positions[sourceIndex] || !positions[targetIndex]) {
                    return null;
                  }
                  const start = positions[sourceIndex];
                  const end = positions[targetIndex];
                  return (
                    <path
                      key={`${depId}:${targetIndex}`}
                      d={edgePath(
                        start.x + NODE_WIDTH,
                        start.y + NODE_HEIGHT / 2,
                        end.x,
                        end.y + NODE_HEIGHT / 2,
                      )}
                      fill="none"
                      stroke={
                        selectedTaskIndex === targetIndex || selectedTaskIndex === sourceIndex
                          ? "#4f46e5"
                          : "#cbd5e1"
                      }
                      strokeWidth={selectedTaskIndex === targetIndex ? 3 : 2}
                      strokeDasharray={selectedTaskIndex === targetIndex ? "0" : "5 7"}
                    />
                  );
                }),
            )}

            {linkState && positions[linkState.sourceIndex] && (
              <path
                d={edgePath(
                  positions[linkState.sourceIndex].x + NODE_WIDTH,
                  positions[linkState.sourceIndex].y + NODE_HEIGHT / 2,
                  linkState.pointerX,
                  linkState.pointerY,
                )}
                fill="none"
                stroke="#4f46e5"
                strokeWidth={3}
                strokeDasharray="8 8"
              />
            )}
          </svg>

          {tasks.map((task, index) => {
            const position = positions[index] ?? { x: 32, y: 32 };
            const deps = task.depsText
              .split(/[,\n]/)
              .map((value) => value.trim())
              .filter(Boolean);
            const isSelected = selectedTaskIndex === index;
            const isLinkSource = linkState?.sourceIndex === index;

            return (
              <article
                key={`${taskDisplayId(task, index)}:${index}`}
                data-workflow-node-index={index}
                className={`absolute rounded-[28px] border p-4 shadow-sm transition ${
                  isSelected
                    ? "border-indigo-200 bg-indigo-50/90 shadow-indigo-100"
                    : isLinkSource
                      ? "border-indigo-200 bg-white shadow-indigo-100"
                      : "border-slate-200 bg-white hover:border-slate-300"
                }`}
                style={{
                  width: NODE_WIDTH,
                  minHeight: NODE_HEIGHT,
                  transform: `translate(${position.x}px, ${position.y}px)`,
                }}
                onClick={() => onSelectTask(index)}
                onPointerDown={(event) => {
                  const target = event.target as HTMLElement;
                  if (target.closest("button")) {
                    return;
                  }
                  onSelectTask(index);
                  const rect = target.closest("article")?.getBoundingClientRect();
                  if (!rect) {
                    return;
                  }
                  setDragState({
                    index,
                    offsetX: event.clientX - rect.left,
                    offsetY: event.clientY - rect.top,
                  });
                }}
              >
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <div className="text-base font-semibold text-slate-900">
                      {taskDisplayId(task, index)}
                    </div>
                    <div className="mt-1 text-sm text-slate-500">
                      {task.role.trim() || "Unassigned role"}
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      title="Start linking from this task"
                      onPointerDown={(event) => {
                        event.preventDefault();
                        event.stopPropagation();
                        onSelectTask(index);
                        const { x, y } = relativePointer(event.clientX, event.clientY);
                        setLinkState({
                          sourceIndex: index,
                          pointerX: x,
                          pointerY: y,
                        });
                      }}
                      className="rounded-xl border border-indigo-200 bg-indigo-50 p-2 text-indigo-700 hover:bg-indigo-100"
                    >
                      <Link2 className="h-4 w-4" />
                    </button>
                    <button
                      type="button"
                      title="Remove task"
                      disabled={tasks.length === 1}
                      onClick={(event) => {
                        event.stopPropagation();
                        onRemoveTask(index);
                      }}
                      className="rounded-xl border border-slate-200 bg-slate-50 p-2 text-slate-500 hover:text-rose-600 disabled:opacity-40"
                    >
                      <Trash2 className="h-4 w-4" />
                    </button>
                  </div>
                </div>

                <div className="mt-4 flex flex-wrap gap-2">
                  <span
                    className={`rounded-full border px-2.5 py-1 text-[11px] font-semibold ${workloadTone(
                      task.workload,
                    )}`}
                  >
                    {task.workload || "general"}
                  </span>
                  <span className="rounded-full border border-slate-200 bg-slate-100 px-2.5 py-1 text-[11px] font-semibold text-slate-600">
                    {deps.length === 0 ? "root" : `${deps.length} deps`}
                  </span>
                </div>

                <div className="mt-4 line-clamp-3 text-sm leading-6 text-slate-600">
                  {task.prompt.trim() || "No prompt yet."}
                </div>

                <div className="mt-4 flex flex-wrap gap-2">
                  {deps.length === 0 ? (
                    <span className="rounded-full border border-dashed border-slate-200 px-2.5 py-1 text-[11px] text-slate-400">
                      No dependencies
                    </span>
                  ) : (
                    deps.map((depId) => (
                      <button
                        key={`${taskDisplayId(task, index)}:${depId}`}
                        type="button"
                        onClick={(event) => {
                          event.stopPropagation();
                          onTasksChange(removeDependency(tasks, index, depId));
                        }}
                        className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-[11px] font-semibold text-slate-600 hover:border-rose-200 hover:text-rose-700"
                      >
                        {depId} ×
                      </button>
                    ))
                  )}
                </div>
              </article>
            );
          })}
        </div>
      </div>
    </div>
  );
}
