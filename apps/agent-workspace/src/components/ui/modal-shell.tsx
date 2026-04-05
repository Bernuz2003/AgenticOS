import { useEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";

interface ModalShellProps {
  isOpen: boolean;
  title: string;
  description?: string;
  onClose: () => void;
  children: ReactNode;
  maxWidthClassName?: string;
}

export function ModalShell({
  isOpen,
  title,
  description,
  onClose,
  children,
  maxWidthClassName = "max-w-6xl",
}: ModalShellProps) {
  useEffect(() => {
    if (!isOpen) {
      return;
    }

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };

    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  }, [isOpen, onClose]);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    const html = document.documentElement;
    const body = document.body;
    const scrollRoot = document.querySelector<HTMLElement>("[data-app-scroll-root]");

    const previousHtmlOverflow = html.style.overflow;
    const previousBodyOverflow = body.style.overflow;
    const previousScrollRootOverflow = scrollRoot?.style.overflow ?? "";
    const previousScrollRootOverscrollBehavior =
      scrollRoot?.style.overscrollBehavior ?? "";

    html.style.overflow = "hidden";
    body.style.overflow = "hidden";
    if (scrollRoot) {
      scrollRoot.style.overflow = "hidden";
      scrollRoot.style.overscrollBehavior = "contain";
    }

    return () => {
      html.style.overflow = previousHtmlOverflow;
      body.style.overflow = previousBodyOverflow;
      if (scrollRoot) {
        scrollRoot.style.overflow = previousScrollRootOverflow;
        scrollRoot.style.overscrollBehavior = previousScrollRootOverscrollBehavior;
      }
    };
  }, [isOpen]);

  if (!isOpen || typeof document === "undefined") {
    return null;
  }

  return createPortal(
    <>
      <div
        className="fixed inset-0 z-[120] bg-slate-950/35 backdrop-blur-sm"
        onClick={onClose}
      />

      <div className="fixed inset-0 z-[130] p-3 md:p-6">
        <div
          className={`mx-auto flex h-full w-full min-h-0 flex-col overflow-hidden rounded-[28px] border border-slate-200 bg-white shadow-2xl ${maxWidthClassName}`}
        >
          <div className="flex items-start justify-between gap-4 border-b border-slate-100 px-6 py-5">
            <div>
              <h2 className="text-lg font-bold text-slate-950">{title}</h2>
              {description ? (
                <p className="mt-1 text-sm text-slate-500">{description}</p>
              ) : null}
            </div>
            <button
              type="button"
              onClick={onClose}
              className="rounded-full p-2 text-slate-400 transition-colors hover:bg-slate-100 hover:text-slate-700"
              aria-label="Close modal"
            >
              <X className="h-5 w-5" />
            </button>
          </div>

          <div className="min-h-0 flex-1 overflow-hidden overscroll-contain px-6 py-5">
            {children}
          </div>
        </div>
      </div>
    </>,
    document.body,
  );
}
