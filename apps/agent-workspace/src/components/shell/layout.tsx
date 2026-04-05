import type { ReactNode } from "react";

import { Sidebar } from "./sidebar";

interface ShellLayoutProps {
  children: ReactNode;
}

export function ShellLayout({ children }: ShellLayoutProps) {
  return (
    <div className="flex min-h-screen bg-white text-slate-900">
      <Sidebar />
      <div className="ml-64 flex min-h-screen flex-1 flex-col">
        <main data-app-scroll-root className="flex-1 overflow-y-auto p-6 md:p-8">
          {children}
        </main>
      </div>
    </div>
  );
}
