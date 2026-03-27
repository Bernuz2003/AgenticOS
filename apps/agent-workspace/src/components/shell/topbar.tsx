import { Link, useLocation } from "react-router-dom";

const TITLES: Array<{ prefix: string; label: string; eyebrow: string }> = [
  { prefix: "/sessions", label: "Chat Sessions", eyebrow: "Chats" },
  { prefix: "/workspace", label: "Workspace", eyebrow: "Session" },
  { prefix: "/workflows", label: "Workflow Builder", eyebrow: "Workflows" },
  { prefix: "/workflow-runs", label: "Workflow Run", eyebrow: "Execution" },
  { prefix: "/jobs", label: "Jobs", eyebrow: "Scheduler" },
  { prefix: "/resources", label: "Resources", eyebrow: "Control Center" },
  { prefix: "/control-center", label: "Control Center", eyebrow: "Resources" },
  { prefix: "/models", label: "Models", eyebrow: "Runtime" },
  { prefix: "/settings", label: "Settings", eyebrow: "Workspace" },
];

function resolveTitle(pathname: string) {
  return TITLES.find((entry) => pathname.startsWith(entry.prefix)) ?? {
    label: "Dashboard",
    eyebrow: "AgenticOS",
  };
}

export function ShellTopbar() {
  const location = useLocation();
  const title = resolveTitle(location.pathname);

  return (
    <header className="sticky top-0 z-10 border-b border-slate-200 bg-white/90 px-8 py-4 backdrop-blur">
      <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
        <div>
          <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-slate-400">
            {title.eyebrow}
          </div>
          <h1 className="mt-1 text-xl font-bold tracking-tight text-slate-900">
            {title.label}
          </h1>
        </div>

        <nav className="flex flex-wrap gap-2">
          {[
            ["/sessions", "Chats"],
            ["/workflows", "Workflows"],
            ["/jobs", "Jobs"],
            ["/resources", "Resources"],
          ].map(([path, label]) => {
            const active = location.pathname.startsWith(path);
            return (
              <Link
                key={path}
                to={path}
                className={`rounded-full border px-3 py-1.5 text-xs font-semibold transition ${
                  active
                    ? "border-indigo-200 bg-indigo-50 text-indigo-700"
                    : "border-slate-200 bg-white text-slate-600 hover:bg-slate-50"
                }`}
              >
                {label}
              </Link>
            );
          })}
        </nav>
      </div>
    </header>
  );
}
