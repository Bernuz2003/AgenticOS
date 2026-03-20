import {
  LayoutDashboard,
  MessageSquare,
  Activity,
  BriefcaseBusiness,
  ServerCog,
  Waypoints,
} from "lucide-react";
import { Link, useLocation } from "react-router-dom";

export function Sidebar() {
  const location = useLocation();

  const navItems = [
    { name: "Dashboard", path: "/", icon: LayoutDashboard },
    { name: "Chats", path: "/sessions", icon: MessageSquare },
    { name: "Workflows", path: "/workflows", icon: Waypoints },
    { name: "Jobs", path: "/jobs", icon: BriefcaseBusiness },
    { name: "Control Center", path: "/control-center", icon: ServerCog },
  ];

  return (
    <aside className="w-64 bg-slate-50 border-r border-slate-200 h-screen flex flex-col fixed left-0 top-0 overflow-y-auto">
      <div className="p-6">
        <h1 className="text-xl font-bold text-slate-900 tracking-tight">
          AgenticOS
        </h1>
        <p className="text-xs text-slate-500 mt-1 uppercase tracking-widest font-semibold">
          Control Plane
        </p>
      </div>

      <nav className="flex-1 px-4 space-y-1">
        {navItems.map((item) => {
          const isActive =
            location.pathname === item.path ||
            (item.path === "/jobs" && location.pathname.startsWith("/workflow-runs/"));
          const Icon = item.icon;
          return (
            <Link
              key={item.path}
              to={item.path}
              className={`flex items-center gap-3 px-3 py-2.5 rounded-md text-sm font-medium transition-colors ${
                isActive
                  ? "bg-indigo-50 text-indigo-700"
                  : "text-slate-600 hover:bg-slate-100 hover:text-slate-900"
              }`}
            >
              <Icon className={`w-5 h-5 ${isActive ? "text-indigo-600" : "text-slate-400"}`} />
              {item.name}
            </Link>
          );
        })}
      </nav>

      <div className="p-4 border-t border-slate-200">
        <div className="flex items-center gap-2 px-3 py-2 text-xs font-medium text-slate-500">
          <Activity className="w-4 h-4" />
          <span>System Status: Online</span>
        </div>
      </div>
    </aside>
  );
}
