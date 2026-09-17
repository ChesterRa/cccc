import type { ComponentPropsWithRef, ReactNode } from "react";
import { X } from "lucide-react";
import { classNames } from "../../utils/classNames";

export function SidePanelButton({
  title,
  className,
  ...props
}: ComponentPropsWithRef<"button"> & { title: string }) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      {...props}
      className={classNames(
        "flex h-8 w-8 max-sm:h-11 max-sm:w-11 shrink-0 items-center justify-center rounded-md text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 disabled:opacity-40 [&_svg]:h-4 [&_svg]:w-4",
        className,
      )}
    />
  );
}

export function SidePanelHeader({
  title,
  subtitle,
  children,
  onClose,
  closeLabel,
}: {
  title: string;
  subtitle?: string;
  children?: ReactNode;
  onClose?: () => void;
  closeLabel: string;
}) {
  return (
    <div
      className="flex min-h-11 shrink-0 items-center gap-1 border-b border-[var(--glass-border-subtle)] px-2 py-1.5"
      data-side-panel-header
    >
      <div className="min-w-0 flex-1 px-1">
        <h2 className="truncate text-sm font-semibold" title={title}>
          {title}
        </h2>
        {subtitle && (
          <p className="truncate text-xs text-[var(--color-text-tertiary)]" title={subtitle}>
            {subtitle}
          </p>
        )}
      </div>
      {children}
      {onClose && (
        <SidePanelButton title={closeLabel} onClick={onClose}>
          <X />
        </SidePanelButton>
      )}
    </div>
  );
}
