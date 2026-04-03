import { LoaderCircle, Send, Square } from "lucide-react";

interface ComposerControlsProps {
  mode: "send" | "stop";
  loading: boolean;
  disabled: boolean;
  title?: string;
  stopRequested?: boolean;
  onStop?: () => void;
}

export function ComposerControls({
  mode,
  loading,
  disabled,
  title,
  stopRequested = false,
  onStop,
}: ComposerControlsProps) {
  if (mode === "stop") {
    return (
      <button
        type="button"
        disabled={disabled}
        onClick={() => {
          if (!disabled) {
            onStop?.();
          }
        }}
        className={`absolute bottom-2 right-2 rounded-xl border p-3 transition-all active:scale-95 disabled:pointer-events-none disabled:opacity-50 ${
          stopRequested
            ? "border-amber-300 bg-amber-50 text-amber-700"
            : "border-rose-200 bg-rose-50 text-rose-700 hover:scale-105 hover:border-rose-300 hover:bg-rose-100"
        }`}
        title={title}
      >
        {loading ? (
          <LoaderCircle className="h-5 w-5 animate-spin" />
        ) : (
          <Square className="h-5 w-5 fill-current" />
        )}
      </button>
    );
  }

  return (
    <button
      type="submit"
      disabled={disabled}
      className="absolute bottom-2 right-2 rounded-xl bg-indigo-600 p-3 text-white transition-all hover:scale-105 hover:bg-indigo-700 active:scale-95 disabled:pointer-events-none disabled:bg-slate-300 disabled:opacity-50"
      title={title ?? "Send Message"}
    >
      {loading ? <LoaderCircle className="h-5 w-5 animate-spin" /> : <Send className="h-5 w-5" />}
    </button>
  );
}
