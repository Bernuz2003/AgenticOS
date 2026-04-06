import type { WorkspaceSnapshot } from "../../../lib/api";
import { formatWorkspaceValue } from "../../../lib/workspace/format";
import { InspectSection } from "./section-shell";

export function PermissionsSection({ snapshot }: { snapshot: WorkspaceSnapshot | null }) {
  const permissions = snapshot?.permissions;

  return (
    <InspectSection
      title="Permissions"
      description="Trust scope, execution permissions and tool boundaries."
    >
      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-semibold text-slate-950">
            {permissions?.trustScope ?? "unknown"}
          </div>
          <div className="mt-1 text-xs text-slate-500">
            caller {snapshot?.toolCaller ?? "n/a"}
          </div>
        </div>
        <span
          className={`status-pill border ${
            permissions?.actionsAllowed
              ? "border-emerald-200 bg-emerald-50 text-emerald-700"
              : "border-slate-200 bg-slate-100 text-slate-600"
          }`}
        >
          {permissions?.actionsAllowed ? "actions enabled" : "actions blocked"}
        </span>
      </div>

      <div className="space-y-3 text-sm">
        <DetailList
          label="Allowed tools"
          values={permissions?.allowedTools ?? []}
          emptyLabel="none"
        />
        <PathGrantList snapshot={snapshot} />
        <DetailList
          label="Path scopes"
          values={permissions?.pathScopes ?? []}
          emptyLabel="none"
        />
        <div>
          <div className="text-xs uppercase tracking-[0.18em] text-slate-400">Tool caller</div>
          <div className="mt-1 font-medium text-slate-900">
            {formatWorkspaceValue(snapshot?.toolCaller)}
          </div>
        </div>
      </div>
    </InspectSection>
  );
}

function PathGrantList({ snapshot }: { snapshot: WorkspaceSnapshot | null }) {
  const grants = snapshot?.permissions?.pathGrants ?? [];

  return (
    <div>
      <div className="text-xs uppercase tracking-[0.18em] text-slate-400">Path grants</div>
      {grants.length === 0 ? (
        <div className="mt-2 text-sm text-slate-500">none</div>
      ) : (
        <div className="mt-2 space-y-2">
          {grants.map((grant) => {
            const key = [
              grant.root,
              grant.accessMode,
              grant.capsule ?? "",
              grant.label ?? "",
              grant.workspaceRelative ? "workspace" : "absolute",
            ].join("|");

            return (
              <div
                key={key}
                className="rounded-2xl border border-slate-200 bg-slate-50 px-3 py-3"
              >
                <div className="break-all font-medium text-slate-900">{grant.root}</div>
                <div className="mt-2 flex flex-wrap gap-2">
                  <GrantMetaPill value={grant.accessMode} />
                  <GrantMetaPill
                    value={grant.workspaceRelative ? "workspace-relative" : "absolute-root"}
                  />
                  {grant.capsule ? <GrantMetaPill value={`capsule:${grant.capsule}`} /> : null}
                  {grant.label ? <GrantMetaPill value={`label:${grant.label}`} /> : null}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function GrantMetaPill({ value }: { value: string }) {
  return (
    <span className="rounded-full border border-slate-200 bg-white px-3 py-1 text-xs font-medium text-slate-700">
      {value}
    </span>
  );
}

function DetailList({
  label,
  values,
  emptyLabel,
}: {
  label: string;
  values: string[];
  emptyLabel: string;
}) {
  return (
    <div>
      <div className="text-xs uppercase tracking-[0.18em] text-slate-400">{label}</div>
      <div className="mt-2 flex flex-wrap gap-2">
        {values.length === 0 ? (
          <span className="text-sm text-slate-500">{emptyLabel}</span>
        ) : (
          values.map((value) => (
            <span
              key={value}
              className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-xs font-medium text-slate-700"
            >
              {value}
            </span>
          ))
        )}
      </div>
    </div>
  );
}
