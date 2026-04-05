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
