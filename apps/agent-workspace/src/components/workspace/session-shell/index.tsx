import type { ReactNode } from "react";

interface SessionShellProps {
  header: ReactNode;
  children: ReactNode;
}

export function SessionShell({
  header,
  children,
}: SessionShellProps) {
  return (
    <div className="mx-auto flex h-[calc(100vh-4rem)] w-full max-w-[1700px] min-h-0 flex-col gap-4">
      {header}
      <div className="flex min-h-0 flex-1">{children}</div>
    </div>
  );
}
