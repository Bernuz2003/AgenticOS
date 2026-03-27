import { useEffect, useMemo, useRef, useState } from "react";
import type { DraftTask } from "../../../lib/workflow-builder";
import {
  addDependency,
  buildInitialNodePositions,
  canAddDependency,
  ensureNodePositions,
} from "../../../lib/workflow-builder/graph";
import { WorkflowCanvas } from "./canvas";
import { NODE_HEIGHT, NODE_WIDTH } from "./node";
import { WorkflowCanvasInspector } from "./inspector";

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
      <WorkflowCanvasInspector
        tasks={tasks}
        linkSourceIndex={linkState?.sourceIndex ?? null}
        onAddTask={onAddTask}
      />

      <WorkflowCanvas
        innerRef={innerRef}
        tasks={tasks}
        positions={positions}
        canvasSize={canvasSize}
        selectedTaskIndex={selectedTaskIndex}
        linkState={linkState}
        onSelectTask={onSelectTask}
        onTasksChange={onTasksChange}
        onStartDrag={(index, clientX, clientY, rect) => {
          setDragState({
            index,
            offsetX: clientX - rect.left,
            offsetY: clientY - rect.top,
          });
        }}
        onStartLink={(index, clientX, clientY) => {
          const { x, y } = relativePointer(clientX, clientY);
          setLinkState({
            sourceIndex: index,
            pointerX: x,
            pointerY: y,
          });
        }}
        onRemoveTask={onRemoveTask}
      />
    </div>
  );
}
