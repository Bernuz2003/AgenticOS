function formatTimestamp(timestampMs: number | null): string {
  if (!timestampMs) {
    return "n/a";
  }
  return new Date(timestampMs).toLocaleString();
}

interface IpcMessageEntry {
  messageId: string;
  messageType: string;
  status: string;
  channel: string | null;
  createdAtMs: number | null;
  deliveredAtMs: number | null;
  consumedAtMs: number | null;
  failedAtMs: number | null;
  senderTask: string | null;
  senderPid: number | null;
  senderAttempt: number | null;
  receiverTask: string | null;
  receiverRole: string | null;
  receiverPid: number | null;
  receiverAttempt: number | null;
  payloadText: string | null;
  payloadPreview: string | null;
}

interface IpcLogProps {
  messages: IpcMessageEntry[];
  emptyMessage: string;
}

export function IpcLog({ messages, emptyMessage }: IpcLogProps) {
  if (messages.length === 0) {
    return (
      <div className="rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-5 py-8 text-sm text-slate-500">
        {emptyMessage}
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {messages.map((message) => (
        <article
          key={message.messageId}
          className="rounded-2xl border border-slate-200 bg-slate-50 p-4"
        >
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex flex-wrap items-center gap-2">
              <span className="rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-indigo-700">
                {message.messageType}
              </span>
              <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                {message.status}
              </span>
              {message.channel && (
                <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                  {message.channel}
                </span>
              )}
            </div>
            <div className="text-[11px] text-slate-500">
              {formatTimestamp(message.createdAtMs)}
            </div>
          </div>

          <div className="mt-3 flex flex-wrap gap-2 text-[11px] text-slate-500">
            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
              from {message.senderTask ?? `pid ${message.senderPid ?? "?"}`}
            </span>
            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
              to{" "}
              {message.receiverTask ??
                (message.receiverRole
                  ? `role ${message.receiverRole}`
                  : `pid ${message.receiverPid ?? "?"}`)}
            </span>
            {message.senderAttempt !== null && (
              <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                sender attempt {message.senderAttempt}
              </span>
            )}
            {message.receiverAttempt !== null && (
              <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                receiver attempt {message.receiverAttempt}
              </span>
            )}
            {message.deliveredAtMs !== null && (
              <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                delivered {formatTimestamp(message.deliveredAtMs)}
              </span>
            )}
            {message.consumedAtMs !== null && (
              <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                consumed {formatTimestamp(message.consumedAtMs)}
              </span>
            )}
            {message.failedAtMs !== null && (
              <span className="rounded-full border border-rose-200 bg-rose-50 px-2.5 py-1 text-rose-700">
                failed {formatTimestamp(message.failedAtMs)}
              </span>
            )}
          </div>

          <div className="mt-3 whitespace-pre-wrap break-words rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm leading-6 text-slate-700">
            {message.payloadText || message.payloadPreview}
          </div>
        </article>
      ))}
    </div>
  );
}
