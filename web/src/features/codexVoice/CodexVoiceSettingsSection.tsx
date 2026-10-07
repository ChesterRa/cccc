import type { ReactNode } from "react";
import { SETTINGS_CARD } from "./codexVoiceSettingsLayout";

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
  /** Let a list run edge to edge inside the card. */
  flush?: boolean;
  children: ReactNode;
}) {
  return (
    <section className={SETTINGS_CARD}>
      <header className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2 border-b border-[var(--glass-border-subtle)] px-4 py-3">
        <div className="min-w-0">
          <h4 className="text-sm font-semibold text-[var(--color-text-primary)]">{title}</h4>
          {description ? (
            <p className="mt-0.5 text-xs leading-5 text-[var(--color-text-muted)]">{description}</p>
          ) : null}
        </div>
        {aside}
      </header>
      <div className={flush ? "" : "p-4"}>{children}</div>
    </section>
  );
}

/** The page title above a settings section; the sidebar names it on narrow panels. */
export function CodexVoiceSettingsHeading({ children }: { children: ReactNode }) {
  return (
    <h4 className="mb-1 hidden text-[22px] font-semibold tracking-tight text-[var(--color-text-primary)] @min-[900px]/voice-settings:block">
      {children}
    </h4>
  );
}
