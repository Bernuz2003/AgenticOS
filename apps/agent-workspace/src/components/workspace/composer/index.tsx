import type { FormEvent } from "react";

import { ComposerControls } from "./controls";
import { ComposerInputBox } from "./input-box";

interface WorkspaceComposerProps {
  value: string;
  placeholder: string;
  disabled: boolean;
  loading: boolean;
  error: string | null;
  onChange: (value: string) => void;
  onSubmit: () => void;
}

export function WorkspaceComposer({
  value,
  placeholder,
  disabled,
  loading,
  error,
  onChange,
  onSubmit,
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
        <ComposerControls loading={loading} disabled={disabled || !value.trim()} />
      </form>
      {error && (
        <div className="mx-auto mt-3 max-w-4xl rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800">
          {error}
        </div>
      )}
    </div>
  );
}
