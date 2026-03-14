import { Cpu, Database, Server, ShieldAlert, Zap } from "lucide-react";
import { useSessionsStore } from "../store/sessions-store";

function formatBytes(bytes: number, decimals = 2) {
  if (bytes === 0) return '0 Bytes';
  const k = 1024;
  const dm = decimals < 0 ? 0 : decimals;
  const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + ' ' + sizes[i];
}

export function ResourcesPage() {
  const { resourceGovernor, memory, runtimeInstances, globalAccounting } = useSessionsStore();

  return (
    <div className="mx-auto max-w-6xl p-8 space-y-8 animate-in fade-in duration-500">
      <header>
        <h1 className="text-3xl font-extrabold tracking-tight text-slate-900">
          System Resources
        </h1>
        <p className="mt-2 text-slate-600">
          Monitor your system's hardware utilization, memory management, and active inference runtimes.
        </p>
      </header>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Resource Governor */}
        <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm flex flex-col">
          <div className="flex items-center gap-3 mb-6">
            <div className="p-2.5 bg-indigo-50 text-indigo-600 rounded-xl">
              <ShieldAlert className="w-5 h-5" />
            </div>
            <div>
              <h2 className="text-lg font-bold text-slate-900">Resource Governor</h2>
              <p className="text-sm text-slate-500">RAM and VRAM Admission Control</p>
            </div>
          </div>

          {!resourceGovernor ? (
            <div className="text-slate-400 text-sm flex-1 flex items-center py-4">Waiting for telemetry...</div>
          ) : (
            <div className="space-y-4">
              <div>
                <div className="flex justify-between text-sm mb-1.5">
                  <span className="font-medium text-slate-700">VRAM Budget</span>
                  <span className="font-mono text-slate-600">{formatBytes(resourceGovernor.vramUsedBytes)} / {formatBytes(resourceGovernor.vramBudgetBytes)}</span>
                </div>
                <div className="w-full bg-slate-100 rounded-full h-2.5 overflow-hidden">
                  <div 
                    className="bg-indigo-500 h-2.5 rounded-full" 
                    style={{ width: `${Math.min(100, (resourceGovernor.vramUsedBytes / (resourceGovernor.vramBudgetBytes || 1)) * 100)}%` }}
                  ></div>
                </div>
              </div>

              <div>
                <div className="flex justify-between text-sm mb-1.5">
                  <span className="font-medium text-slate-700">RAM Budget</span>
                  <span className="font-mono text-slate-600">{formatBytes(resourceGovernor.ramUsedBytes)} / {formatBytes(resourceGovernor.ramBudgetBytes)}</span>
                </div>
                <div className="w-full bg-slate-100 rounded-full h-2.5 overflow-hidden">
                  <div 
                    className="bg-teal-500 h-2.5 rounded-full" 
                    style={{ width: `${Math.min(100, (resourceGovernor.ramUsedBytes / (resourceGovernor.ramBudgetBytes || 1)) * 100)}%` }}
                  ></div>
                </div>
              </div>

              <div className="grid grid-cols-2 gap-4 mt-6">
                 <div className="bg-slate-50 p-4 rounded-2xl border border-slate-100">
                    <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Queue Depth</p>
                    <p className="text-2xl font-bold font-mono text-slate-800">{resourceGovernor.pendingQueueDepth}</p>
                 </div>
                 <div className="bg-slate-50 p-4 rounded-2xl border border-slate-100">
                    <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Loader Status</p>
                    <p className={`text-sm font-bold mt-2 ${resourceGovernor.loaderBusy ? 'text-amber-600' : 'text-emerald-600'}`}>
                      {resourceGovernor.loaderBusy ? resourceGovernor.loaderReason || 'Busy' : 'Idle'}
                    </p>
                 </div>
              </div>
            </div>
          )}
        </section>

        {/* Memory Management */}
        <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm flex flex-col">
          <div className="flex items-center gap-3 mb-6">
            <div className="p-2.5 bg-rose-50 text-rose-600 rounded-xl">
              <Database className="w-5 h-5" />
            </div>
            <div>
              <h2 className="text-lg font-bold text-slate-900">Virtual Memory</h2>
              <p className="text-sm text-slate-500">Paging and Context Residency</p>
            </div>
          </div>

          {!memory ? (
            <div className="text-slate-400 text-sm flex-1 flex items-center py-4">Waiting for telemetry...</div>
          ) : (
            <div className="grid grid-cols-2 lg:grid-cols-3 gap-4">
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Tracked PIDs</p>
                  <p className="text-xl font-bold font-mono text-slate-800">{memory.trackedPids}</p>
               </div>
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Parked PIDs</p>
                  <p className="text-xl font-bold font-mono text-slate-800">{memory.parkedPids}</p>
               </div>
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Free Blocks</p>
                  <p className="text-xl font-bold font-mono text-slate-800">{memory.freeBlocks} / {memory.totalBlocks}</p>
               </div>
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Alloc Tensors</p>
                  <p className="text-xl font-bold font-mono text-slate-800">{memory.allocatedTensors}</p>
               </div>
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Alloc Bytes</p>
                  <p className="text-lg font-bold font-mono text-slate-800">{formatBytes(memory.allocBytes)}</p>
               </div>
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Evictions</p>
                  <p className="text-xl font-bold font-mono text-slate-800">{memory.evictions}</p>
               </div>
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Swap Faults</p>
                  <p className="text-xl font-bold font-mono text-rose-600">{memory.swapFaults}</p>
               </div>
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">OOM Events</p>
                  <p className="text-xl font-bold font-mono text-rose-600">{memory.oomEvents}</p>
               </div>
            </div>
          )}
        </section>

        {/* Global Accounting */}
        <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm md:col-span-2">
          <div className="flex items-center gap-3 mb-6">
            <div className="p-2.5 bg-emerald-50 text-emerald-600 rounded-xl">
              <Zap className="w-5 h-5" />
            </div>
            <div>
              <h2 className="text-lg font-bold text-slate-900">Global Accounting</h2>
              <p className="text-sm text-slate-500">Total system wide inference metrics</p>
            </div>
          </div>

          {!globalAccounting ? (
            <div className="text-slate-400 text-sm py-4">Waiting for telemetry...</div>
          ) : (
            <div className="flex flex-wrap gap-x-12 gap-y-6">
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Input Tokens</p>
                  <p className="text-2xl font-bold font-mono text-slate-800">
                    {globalAccounting.inputTokensTotal.toLocaleString()}
                  </p>
               </div>
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Output Tokens</p>
                  <p className="text-2xl font-bold font-mono text-slate-800">
                    {globalAccounting.outputTokensTotal.toLocaleString()}
                  </p>
               </div>
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Total Tokens</p>
                  <p className="text-2xl font-bold font-mono text-slate-800">
                    {(globalAccounting.inputTokensTotal + globalAccounting.outputTokensTotal).toLocaleString()}
                  </p>
               </div>
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Est. Cost</p>
                  <p className="text-2xl font-bold font-mono text-emerald-600">
                    ${globalAccounting.estimatedCostUsd.toFixed(4)}
                  </p>
               </div>
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Total Requests</p>
                  <p className="text-2xl font-bold font-mono text-slate-800">
                    {globalAccounting.requestsTotal + globalAccounting.streamRequestsTotal}
                  </p>
               </div>
               <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-1">Errors</p>
                  <p className="text-2xl font-bold font-mono text-rose-600">
                    {globalAccounting.rateLimitErrors + globalAccounting.authErrors + globalAccounting.transportErrors}
                  </p>
               </div>
            </div>
          )}
        </section>

        {/* Active Runtimes */}
        <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm md:col-span-2">
          <div className="flex items-center gap-3 mb-6">
            <div className="p-2.5 bg-sky-50 text-sky-600 rounded-xl">
              <Cpu className="w-5 h-5" />
            </div>
            <div>
              <h2 className="text-lg font-bold text-slate-900">Active Runtimes</h2>
              <p className="text-sm text-slate-500">Models currently resident in memory</p>
            </div>
          </div>

          {runtimeInstances.length === 0 ? (
            <div className="rounded-2xl border border-slate-100 bg-slate-50 px-6 py-10 text-center">
               <Server className="w-10 h-10 text-slate-300 mx-auto mb-3" />
               <p className="text-slate-500 font-medium text-sm">No active runtimes</p>
               <p className="text-slate-400 text-xs mt-1">Load a model from the Dashboard to start a resident loop.</p>
            </div>
          ) : (
            <div className="space-y-4">
              {runtimeInstances.map((instance) => (
                <div key={instance.runtimeId} className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 p-5 rounded-2xl border border-slate-200 bg-white shadow-[0_2px_10px_-4px_rgba(0,0,0,0.05)]">
                   <div>
                     <div className="flex items-center gap-3">
                       <h3 className="font-bold text-slate-900">{instance.logicalModelId}</h3>
                       <span className="rounded-full bg-emerald-100 px-2.5 py-0.5 text-[10px] font-bold uppercase tracking-wider text-emerald-700">
                         {instance.state}
                       </span>
                       {instance.pinned && (
                         <span className="rounded-full bg-slate-100 px-2.5 py-0.5 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                           Pinned
                         </span>
                       )}
                     </div>
                     <p className="text-sm text-slate-500 mt-1 flex items-center gap-2">
                       <kbd className="font-mono text-xs bg-slate-50 border border-slate-200 rounded px-1">{instance.backendClass}</kbd>
                       <span>&middot;</span>
                       <span>Runtime: {instance.runtimeId}</span>
                     </p>
                   </div>
                   
                   <div className="flex gap-6 items-center">
                     <div className="text-right">
                       <p className="text-xs font-semibold uppercase tracking-wider text-slate-500">Active PIDs</p>
                       <p className="font-mono font-bold text-slate-800">{instance.activePidCount}</p>
                     </div>
                     <div className="text-right">
                       <p className="text-xs font-semibold uppercase tracking-wider text-slate-500">VRAM Rez</p>
                       <p className="font-mono font-bold text-slate-800">{formatBytes(instance.reservationVramBytes)}</p>
                     </div>
                     <div className="text-right">
                       <p className="text-xs font-semibold uppercase tracking-wider text-slate-500">RAM Rez</p>
                       <p className="font-mono font-bold text-slate-800">{formatBytes(instance.reservationRamBytes)}</p>
                     </div>
                   </div>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
