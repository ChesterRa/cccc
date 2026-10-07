import { FolderIcon, LinkIcon, UploadIcon } from "lucide-react";
import { classNames } from "../../utils/classNames";

export type PresentationPinSource = "url" | "workspace" | "upload";

const SOURCE_ICONS = { url: LinkIcon, workspace: FolderIcon, upload: UploadIcon };

/** Radio cards for where a pinned card comes from: stacked on phones, a row on wider screens. */
export function PresentationPinSourcePicker({
  value,
  legend,
  labels,
  onChange,
}: {
  value: PresentationPinSource;
  legend: string;
  labels: Record<PresentationPinSource, string>;
  onChange: (source: PresentationPinSource) => void;
}) {
  return (
    <fieldset className="m-0 min-w-0 border-0 p-0">
      <legend className="mb-2 p-0 text-[13px] font-medium text-[var(--color-text-secondary)]">
        {legend}
      </legend>
      <div className="grid gap-2 sm:grid-cols-3">
        {(["url", "workspace", "upload"] as const).map((source) => {
          const checked = value === source;
          const Icon = SOURCE_ICONS[source];
          return (
            <label
              key={source}
              className={classNames(
                "relative flex min-h-[52px] cursor-pointer items-center gap-3 rounded-xl border bg-[var(--color-bg-primary)] px-3.5 transition-colors has-[:focus-visible]:outline has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-[var(--color-border-focus)]",
                checked
                  ? "border-[var(--color-text-primary)] shadow-[inset_0_0_0_0.5px_var(--color-text-primary)]"
                  : "border-[var(--glass-border-subtle)] hover:border-[var(--color-border-primary)]",
              )}
            >
              <input
                type="radio"
                name="presentation-pin-source"
                value={source}
                checked={checked}
                onChange={() => onChange(source)}
                className="sr-only"
              />
              <span
                className={classNames(
                  "flex h-8 w-8 flex-none items-center justify-center rounded-lg",
                  checked
                    ? "bg-[var(--color-text-primary)] text-[var(--color-bg-primary)]"
                    : "bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]",
                )}
                aria-hidden="true"
              >
                <Icon className="h-4 w-4" />
              </span>
              <span
                className={classNames(
                  "min-w-0 flex-1 text-[15px] sm:text-sm",
                  checked
                    ? "font-semibold text-[var(--color-text-primary)]"
                    : "text-[var(--color-text-primary)]",
                )}
              >
                {labels[source]}
              </span>
              <span
                className={classNames(
                  "h-5 w-5 flex-none rounded-full",
                  checked
                    ? "border-[6px] border-[var(--color-text-primary)]"
                    : "border-[1.5px] border-[var(--color-border-primary)]",
                )}
                aria-hidden="true"
              />
            </label>
          );
        })}
      </div>
    </fieldset>
  );
}
