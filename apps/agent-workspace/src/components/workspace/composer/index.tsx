import type { FormEvent } from "react";

import { ComposerControls } from "./controls";
import { ComposerInputBox } from "./input-box";

interface WorkspaceComposerProps {
  value: string;
  placeholder: string;
  disabled: boolean;
  loading: boolean;
  error: string | null;
  showStopButton: boolean;
  stopLoading: boolean;
  stopDisabled: boolean;
  stopRequested: boolean;
  stopTitle: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
  onStop: () => void;
}

export function WorkspaceComposer({
  value,
  placeholder,
  disabled,
  loading,
  error,
  showStopButton,
  stopLoading,
  stopDisabled,
  stopRequested,
  stopTitle,
  onChange,
  onSubmit,
  onStop,
}: WorkspaceComposerProps) {
  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (disabled || !value.trim()) {
      return;
    }
    onSubmit();
  }

  return (
    <div className="shrink-0 border-t border-slate-200 bg-white p-4">
      <form onSubmit={handleSubmit} className="relative mx-auto flex max-w-4xl items-end gap-2">
        <ComposerInputBox
          value={value}
          placeholder={placeholder}
          disabled={disabled}
          onChange={onChange}
          onSubmit={() => {
            if (!disabled && value.trim()) {
              onSubmit();
            }
          }}
        />
        <ComposerControls
          mode={showStopButton ? "stop" : "send"}
          loading={showStopButton ? stopLoading : loading}
          disabled={showStopButton ? stopDisabled : disabled || !value.trim()}
          title={showStopButton ? stopTitle : "Invia messaggio"}
          stopRequested={stopRequested}
          onStop={onStop}
        />
      </form>
      {error && (
        <div className="mx-auto mt-3 max-w-4xl rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800">
          {error}
        </div>
      )}
    </div>
  );
}
