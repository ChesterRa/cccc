import type { ReactNode } from "react";

export function CodexVoiceSettingsSection({
  title,
  description,
  aside,
  flush = false,
  children,
}: {
  title: ReactNode;
  description?: ReactNode;
  aside?: ReactNode;
  /** Lists provide their own spacing. */
  flush?: boolean;
  children: ReactNode;
}) {
  return (
    <section className="min-w-0 space-y-4">
      <header className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2">
        <div className="min-w-0">
          <h4 className="text-sm font-semibold text-[var(--color-text-primary)]">{title}</h4>
          {description ? (
            <p className="mt-0.5 text-xs leading-5 text-[var(--color-text-muted)]">{description}</p>
          ) : null}
        </div>
        {aside}
      </header>
      <div className={flush ? "" : "min-w-0"}>{children}</div>
    </section>
  );
}
