import { Link2, ShieldCheck, TriangleAlert } from "lucide-react";

import type { McpStatus } from "../../lib/api";

interface McpSectionProps {
  mcp: McpStatus | null;
}

function healthTone(health: string): string {
  switch (health) {
    case "ready":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "degraded":
      return "border-amber-200 bg-amber-50 text-amber-700";
    default:
      return "border-slate-200 bg-slate-100 text-slate-600";
  }
}

export function McpSection({ mcp }: McpSectionProps) {
  if (!mcp || mcp.servers.length === 0) {
    return (
      <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
        <div className="flex items-start gap-3">
          <Link2 className="mt-0.5 h-5 w-5 text-slate-400" />
          <div>
            <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
              MCP Interop
            </div>
            <h2 className="mt-2 text-xl font-bold text-slate-900">
              External interop bridge
            </h2>
            <p className="mt-2 text-sm text-slate-500">
              MCP is disabled or there are no enabled MCP server configurations
              in the current kernel snapshot.
            </p>
          </div>
        </div>
      </section>
    );
  }

  return (
    <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
      <div className="flex items-start justify-between gap-4">
        <div>
          <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
            MCP Interop
          </div>
          <h2 className="mt-2 text-xl font-bold text-slate-900">
            Configured servers, discovery and trust gates
          </h2>
          <p className="mt-2 text-sm text-slate-500">
            MCP-backed capabilities stay edge-only and remain visible as
            governed AgenticOS tools with explicit trust, approval and roots
            constraints.
          </p>
        </div>
        <div className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
          {mcp.servers.length} servers
        </div>
      </div>

      <div className="mt-6 space-y-4">
        {mcp.servers.map((server) => (
          <article
            key={server.serverId}
            className="rounded-2xl border border-slate-200 bg-slate-50 p-5"
          >
            <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
              <div>
                <div className="flex flex-wrap items-center gap-2">
                  <h3 className="text-lg font-semibold text-slate-900">
                    {server.label || server.serverId}
                  </h3>
                  <span
                    className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${healthTone(
                      server.health,
                    )}`}
                  >
                    {server.health}
                  </span>
                  <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                    {server.transport}
                  </span>
                </div>
                <div className="mt-2 text-sm text-slate-500">
                  server_id: {server.serverId} · tool prefix: {server.toolPrefix}
                </div>
              </div>

              <div className="grid grid-cols-2 gap-3 text-xs xl:min-w-[22rem]">
                <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                  <div className="text-slate-500">Trust / auth</div>
                  <div className="mt-1 font-medium text-slate-900">
                    {server.trustLevel} / {server.authMode}
                  </div>
                </div>
                <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                  <div className="text-slate-500">Latency</div>
                  <div className="mt-1 font-medium text-slate-900">
                    {server.lastLatencyMs !== null
                      ? `${server.lastLatencyMs} ms`
                      : "n/a"}
                  </div>
                </div>
                <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                  <div className="text-slate-500">Approval / roots</div>
                  <div className="mt-1 font-medium text-slate-900">
                    {server.approvalRequired ? "approval" : "direct"} /{" "}
                    {server.rootsEnabled ? "roots on" : "roots off"}
                  </div>
                </div>
                <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                  <div className="text-slate-500">Tools / prompts / resources</div>
                  <div className="mt-1 font-medium text-slate-900">
                    {server.discoveredTools.length} / {server.prompts.length} /{" "}
                    {server.resources.length}
                  </div>
                </div>
              </div>
            </div>

            {server.lastError ? (
              <div className="mt-4 flex items-start gap-2 rounded-xl border border-amber-200 bg-amber-50 px-3 py-3 text-sm text-amber-800">
                <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />
                <span>{server.lastError}</span>
              </div>
            ) : null}

            <div className="mt-4 flex flex-wrap gap-2">
              <span className="rounded-full border border-slate-200 bg-white px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
                connected: {server.connected ? "yes" : "no"}
              </span>
              <span className="rounded-full border border-slate-200 bg-white px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
                default allowlisted: {server.defaultAllowlisted ? "yes" : "no"}
              </span>
              {server.exposedTools.length > 0 ? (
                <span className="rounded-full border border-slate-200 bg-white px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
                  exposed filter: {server.exposedTools.join(", ")}
                </span>
              ) : null}
            </div>

            <div className="mt-5 grid gap-4 xl:grid-cols-[minmax(0,1.1fr)_minmax(0,0.9fr)]">
              <div className="rounded-2xl border border-slate-200 bg-white p-4">
                <div className="flex items-center gap-2 text-sm font-semibold text-slate-900">
                  <ShieldCheck className="h-4 w-4 text-indigo-500" />
                  MCP-backed tool registrations
                </div>
                {server.discoveredTools.length === 0 ? (
                  <div className="mt-3 text-sm text-slate-500">
                    No governed AgenticOS tool is currently exposed from this
                    server.
                  </div>
                ) : (
                  <div className="mt-3 space-y-3">
                    {server.discoveredTools.map((tool) => (
                      <div
                        key={`${server.serverId}:${tool.agenticToolName}`}
                        className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3"
                      >
                        <div className="flex flex-wrap items-center gap-2">
                          <div className="font-medium text-slate-900">
                            {tool.agenticToolName}
                          </div>
                          <span className="text-xs text-slate-400">→</span>
                          <div className="text-sm text-slate-600">
                            {tool.targetName}
                          </div>
                        </div>
                        <div className="mt-2 text-sm text-slate-500">
                          {tool.description || "No description from MCP server."}
                        </div>
                        <div className="mt-3 flex flex-wrap gap-2">
                          <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                            {tool.readOnlyHint ? "read_only" : "read_write"}
                          </span>
                          {tool.dangerous ? (
                            <span className="rounded-full border border-rose-200 bg-rose-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-rose-700">
                              dangerous
                            </span>
                          ) : null}
                          {tool.approvalRequired ? (
                            <span className="rounded-full border border-amber-200 bg-amber-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-amber-700">
                              approval
                            </span>
                          ) : null}
                          {tool.idempotentHint ? (
                            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                              idempotent
                            </span>
                          ) : null}
                          {tool.openWorldHint ? (
                            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                              open_world
                            </span>
                          ) : null}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              <div className="rounded-2xl border border-slate-200 bg-white p-4">
                <div className="text-sm font-semibold text-slate-900">
                  Prompts & resources
                </div>
                <div className="mt-3 grid gap-3 text-sm">
                  <div className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3">
                    <div className="text-xs font-bold uppercase tracking-wider text-slate-500">
                      Prompts
                    </div>
                    <div className="mt-2 text-slate-900">
                      {server.prompts.length > 0
                        ? server.prompts
                            .slice(0, 5)
                            .map((prompt) => prompt.title || prompt.name)
                            .join(", ")
                        : "No prompts discovered"}
                    </div>
                  </div>
                  <div className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3">
                    <div className="text-xs font-bold uppercase tracking-wider text-slate-500">
                      Resources
                    </div>
                    <div className="mt-2 text-slate-900">
                      {server.resources.length > 0
                        ? server.resources
                            .slice(0, 4)
                            .map((resource) => resource.title || resource.name)
                            .join(", ")
                        : "No resources discovered"}
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}
