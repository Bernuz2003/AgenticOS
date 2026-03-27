import type { DraftTask } from "../../../lib/workflow-builder";
import { taskDisplayId } from "../../../lib/workflow-builder/graph";
import { WorkflowCanvasEdge } from "./edge";
import { NODE_HEIGHT, NODE_WIDTH, WorkflowCanvasNode } from "./node";

interface WorkflowCanvasProps {
  innerRef: React.RefObject<HTMLDivElement | null>;
  tasks: DraftTask[];
  positions: Array<{ x: number; y: number }>;
  canvasSize: { width: number; height: number };
  selectedTaskIndex: number | null;
  linkState: { sourceIndex: number; pointerX: number; pointerY: number } | null;
  onSelectTask: (index: number) => void;
  onTasksChange: (tasks: DraftTask[]) => void;
  onStartDrag: (index: number, clientX: number, clientY: number, rect: DOMRect) => void;
  onStartLink: (index: number, clientX: number, clientY: number) => void;
  onRemoveTask: (index: number) => void;
}

export function WorkflowCanvas({
  innerRef,
  tasks,
  positions,
  canvasSize,
  selectedTaskIndex,
  linkState,
  onSelectTask,
  onTasksChange,
  onStartDrag,
  onStartLink,
  onRemoveTask,
}: WorkflowCanvasProps) {
  return (
    <div className="overflow-auto rounded-[28px] border border-slate-200 bg-white">
      <div ref={innerRef} className="relative" style={{ width: canvasSize.width, height: canvasSize.height }}>
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
                  <WorkflowCanvasEdge
                    key={`${depId}:${targetIndex}`}
                    startX={start.x + NODE_WIDTH}
                    startY={start.y + NODE_HEIGHT / 2}
                    endX={end.x}
                    endY={end.y + NODE_HEIGHT / 2}
                    active={
                      selectedTaskIndex === targetIndex || selectedTaskIndex === sourceIndex
                    }
                  />
                );
              }),
          )}

          {linkState && positions[linkState.sourceIndex] && (
            <WorkflowCanvasEdge
              startX={positions[linkState.sourceIndex].x + NODE_WIDTH}
              startY={positions[linkState.sourceIndex].y + NODE_HEIGHT / 2}
              endX={linkState.pointerX}
              endY={linkState.pointerY}
              active
              dashed
            />
          )}
        </svg>

        {tasks.map((task, index) => (
          <WorkflowCanvasNode
            key={`${taskDisplayId(task, index)}:${index}`}
            task={task}
            index={index}
            position={positions[index] ?? { x: 32, y: 32 }}
            tasks={tasks}
            selected={selectedTaskIndex === index}
            linkSource={linkState?.sourceIndex === index}
            onSelect={onSelectTask}
            onStartDrag={onStartDrag}
            onStartLink={onStartLink}
            onRemoveTask={onRemoveTask}
            onTasksChange={onTasksChange}
          />
        ))}
      </div>
    </div>
  );
}
