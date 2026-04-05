import { useState, type ReactNode } from "react";

import { ModalShell } from "./modal-shell";

interface PreviewRecordListProps<Item> {
  items: readonly Item[];
  previewLimit?: number;
  emptyState: ReactNode;
  getKey: (item: Item, index: number) => string;
  renderItem: (item: Item, index: number) => ReactNode;
  modalTitle: string;
  modalDescription?: string;
  modalMaxWidthClassName?: string;
}

export function PreviewRecordList<Item>({
  items,
  previewLimit = 10,
  emptyState,
  getKey,
  renderItem,
  modalTitle,
  modalDescription,
  modalMaxWidthClassName,
}: PreviewRecordListProps<Item>) {
  const [isModalOpen, setIsModalOpen] = useState(false);

  if (items.length === 0) {
    return emptyState;
  }

  const previewItems = items.slice(0, previewLimit);
  const hiddenCount = items.length - previewItems.length;

  return (
    <>
      <div className="space-y-4">
        {previewItems.map((item, index) => (
          <div key={getKey(item, index)}>{renderItem(item, index)}</div>
        ))}
      </div>

      {hiddenCount > 0 ? (
        <div className="mt-5 flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3">
          <div className="text-sm text-slate-500">
            Mostrando gli ultimi {previewItems.length} di {items.length} record.
          </div>
          <button
            type="button"
            onClick={() => setIsModalOpen(true)}
            className="rounded-xl border border-slate-200 bg-white px-4 py-2 text-sm font-semibold text-slate-700 transition hover:bg-slate-100"
          >
            Visualizza tutto
          </button>
        </div>
      ) : null}

      <ModalShell
        isOpen={isModalOpen}
        title={modalTitle}
        description={modalDescription}
        onClose={() => setIsModalOpen(false)}
        maxWidthClassName={modalMaxWidthClassName}
      >
        <div className="h-full overflow-y-auto pr-1">
          <div className="space-y-4">
            {items.map((item, index) => (
              <div key={getKey(item, index)}>{renderItem(item, index)}</div>
            ))}
          </div>
        </div>
      </ModalShell>
    </>
  );
}
