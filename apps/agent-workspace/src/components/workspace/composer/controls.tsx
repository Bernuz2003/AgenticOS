import { LoaderCircle, Send } from "lucide-react";

interface ComposerControlsProps {
  loading: boolean;
  disabled: boolean;
}

export function ComposerControls({
  loading,
  disabled,
}: ComposerControlsProps) {
  return (
    <button
      type="submit"
      disabled={disabled}
      className="absolute bottom-2 right-2 rounded-xl bg-indigo-600 p-3 text-white transition-all hover:scale-105 hover:bg-indigo-700 active:scale-95 disabled:pointer-events-none disabled:bg-slate-300 disabled:opacity-50"
      title="Send Message"
    >
      {loading ? <LoaderCircle className="h-5 w-5 animate-spin" /> : <Send className="h-5 w-5" />}
    </button>
  );
}
