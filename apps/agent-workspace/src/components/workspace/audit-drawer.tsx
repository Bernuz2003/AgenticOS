import { useRef, useEffect } from "react";
import { X, Activity } from "lucide-react";
import type { WorkspaceSnapshot } from "../../lib/api";

interface AuditDrawerProps {
  isOpen: boolean;
  onClose: () => void;
  snapshot: WorkspaceSnapshot | null;
}

export function AuditDrawer({ isOpen, onClose, snapshot }: AuditDrawerProps) {
  const drawerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    if (isOpen) {
      document.addEventListener("keydown", handleEscape);
    }
    return () => document.removeEventListener("keydown", handleEscape);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <>
      <div 
        className="fixed inset-0 bg-slate-900/20 backdrop-blur-sm z-40 transition-opacity" 
        onClick={onClose}
      />
      
      <div 
        ref={drawerRef}
        className="fixed right-0 top-0 h-screen w-full max-w-md bg-white shadow-2xl z-50 flex flex-col transform transition-transform duration-300 ease-in-out border-l border-slate-200"
      >
        <div className="flex items-center justify-between px-6 py-5 border-b border-slate-100">
          <div className="flex items-center gap-2 text-indigo-900">
            <Activity className="w-5 h-5" />
            <h2 className="text-lg font-bold">Technical Audit Timeline</h2>
          </div>
          <button 
            onClick={onClose}
            className="p-2 text-slate-400 hover:text-slate-600 hover:bg-slate-100 rounded-full transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-6 space-y-6">
          {!snapshot?.auditEvents || snapshot.auditEvents.length === 0 ? (
            <div className="text-center py-10 text-slate-500 text-sm">
              Nessun evento tecnico registrato per questa sessione.
            </div>
          ) : (
            <div className="relative border-l-2 border-slate-100 ml-3 space-y-8">
              {snapshot.auditEvents.map((event, idx) => (
                <div key={`${event.recordedAtMs}-${idx}`} className="relative pl-6">
                  <div className="absolute w-3 h-3 bg-indigo-500 rounded-full -left-[7px] top-1.5 ring-4 ring-white" />
                  
                  <div className="text-xs font-semibold text-slate-400 mb-1">
                    {new Date(event.recordedAtMs).toLocaleTimeString()}
                  </div>
                  
                  <div className="bg-slate-50 border border-slate-100 rounded-2xl p-4 shadow-sm">
                    <div className="flex items-start justify-between gap-2 mb-2">
                      <h4 className="font-semibold text-slate-900 text-sm">{event.title}</h4>
                      <span className="text-[10px] uppercase tracking-wider font-bold text-slate-500 bg-white px-2 py-0.5 rounded-md border border-slate-200 whitespace-nowrap">
                        {event.kind}
                      </span>
                    </div>
                    
                    <p className="text-sm text-slate-600 font-mono text-xs overflow-x-auto">
                      {event.detail}
                    </p>
                    
                    {event.pid && (
                      <div className="mt-3 text-[10px] uppercase font-semibold text-slate-400 tracking-wider">
                        PID: {event.pid}
                      </div>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </>
  );
}
