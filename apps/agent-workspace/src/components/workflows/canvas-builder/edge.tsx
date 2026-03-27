interface WorkflowCanvasEdgeProps {
  startX: number;
  startY: number;
  endX: number;
  endY: number;
  active: boolean;
  dashed?: boolean;
}

export function edgePath(
  startX: number,
  startY: number,
  endX: number,
  endY: number,
): string {
  const delta = Math.max(72, Math.abs(endX - startX) * 0.45);
  return `M ${startX} ${startY} C ${startX + delta} ${startY}, ${endX - delta} ${endY}, ${endX} ${endY}`;
}

export function WorkflowCanvasEdge({
  startX,
  startY,
  endX,
  endY,
  active,
  dashed,
}: WorkflowCanvasEdgeProps) {
  return (
    <path
      d={edgePath(startX, startY, endX, endY)}
      fill="none"
      stroke={active ? "#4f46e5" : "#cbd5e1"}
      strokeWidth={active ? 3 : 2}
      strokeDasharray={dashed ? "8 8" : active ? "0" : "5 7"}
    />
  );
}
