export type DebugTabId =
  | "overview"
  | "manifest"
  | "tool-calls"
  | "audit"
  | "context"
  | "replay";

interface DebugTabsProps {
  activeTab: DebugTabId;
  onTabChange: (tab: DebugTabId) => void;
}

const DEBUG_TABS: Array<{ id: DebugTabId; label: string }> = [
  { id: "overview", label: "Overview" },
  { id: "manifest", label: "Manifest" },
  { id: "tool-calls", label: "Tool Calls" },
  { id: "audit", label: "Session Audit" },
  { id: "context", label: "Context Snapshot" },
  { id: "replay", label: "Replay / Fork" },
];

export function DebugTabs({ activeTab, onTabChange }: DebugTabsProps) {
  return (
    <nav className="flex flex-wrap gap-2 border-b border-slate-200 px-4 py-4" aria-label="Debug tabs">
      {DEBUG_TABS.map((tab) => (
        <button
          key={tab.id}
          type="button"
          onClick={() => onTabChange(tab.id)}
          className={`rounded-2xl px-3 py-2 text-sm font-semibold transition ${
            activeTab === tab.id
              ? "bg-slate-950 text-white"
              : "bg-slate-100 text-slate-600 hover:bg-slate-200 hover:text-slate-950"
          }`}
          aria-pressed={activeTab === tab.id}
        >
          {tab.label}
        </button>
      ))}
    </nav>
  );
}
