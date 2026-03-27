interface ComposerInputBoxProps {
  value: string;
  placeholder: string;
  disabled: boolean;
  onChange: (value: string) => void;
  onSubmit: () => void;
}

export function ComposerInputBox({
  value,
  placeholder,
  disabled,
  onChange,
  onSubmit,
}: ComposerInputBoxProps) {
  return (
    <textarea
      value={value}
      onChange={(event) => onChange(event.target.value)}
      placeholder={placeholder}
      disabled={disabled}
      className="w-full max-h-60 min-h-[56px] resize-y rounded-2xl border border-slate-300 bg-white px-5 py-4 pr-16 text-[15px] leading-relaxed text-slate-900 shadow-sm outline-none transition-all focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10 disabled:bg-slate-50 disabled:text-slate-500"
      onKeyDown={(event) => {
        if (event.key === "Enter" && !event.shiftKey) {
          event.preventDefault();
          onSubmit();
        }
      }}
    />
  );
}
