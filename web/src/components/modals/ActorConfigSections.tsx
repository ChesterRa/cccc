import { type ReactNode, useId } from "react";
import { ChevronRightIcon } from "../Icons";

interface ActorConfigFormProps {
  identityLabel: string;
  runtimeLabel: string;
  /** Who the Agent is: avatar, name, role. */
  identity: ReactNode;
  /** How the Agent runs. */
  runtime: ReactNode;
  /** Rows under identity in the left column; an absent row leaves no divider behind. */
  directory?: ReactNode;
  notes?: ReactNode;
  advanced?: ReactNode;
}

/**
 * The Agent form as one flat sheet. On wide screens the left column holds who the Agent is,
 * where it works and its notes, and the right column how it runs; advanced settings span
 * both below. Narrow screens stack identity, runtime, directory, notes, advanced.
 */
export function ActorConfigForm({
  identityLabel,
  runtimeLabel,
  identity,
  runtime,
  directory,
  notes,
  advanced,
}: ActorConfigFormProps) {
  const rows = [
    { id: "directory", node: directory },
    { id: "notes", node: notes },
    { id: "advanced", node: advanced },
  ].filter((row) => row.node != null && row.node !== false && row.node !== "");
  const row = (id: string) => rows.find((item) => item.id === id);
  const directoryRow = row("directory");
  const notesRow = row("notes");
  const advancedRow = row("advanced");
  // Wide screens: who the Agent is and where it works stack on the left, how it runs fills
  // the right, so neither column is left mostly empty. The document order stays identity,
  // runtime, directory, notes, which is also the order a narrow screen stacks them in.
  const leftRow = "min-w-0 border-t border-[var(--glass-border-subtle)] py-4 lg:col-start-1";
  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div
        className="grid min-w-0 flex-1 grid-cols-1 gap-x-8 lg:grid-cols-[minmax(0,5fr)_minmax(0,6fr)] lg:grid-rows-[auto_auto_minmax(0,1fr)]"
        data-actor-config-grid
      >
        <section aria-label={identityLabel} className="min-w-0 pb-5 lg:col-start-1 lg:row-start-1">
          {identity}
        </section>
        <section
          aria-label={runtimeLabel}
          className="min-w-0 pb-5 lg:col-start-2 lg:row-span-3 lg:row-start-1"
        >
          {runtime}
        </section>
        {directoryRow ? (
          <div data-actor-config-row="directory" className={`${leftRow} lg:row-start-2`}>
            {directoryRow.node}
          </div>
        ) : null}
        {notesRow ? (
          <div
            data-actor-config-row="notes"
            // The notes take whatever height the left column has left.
            className={`${leftRow} flex flex-col lg:row-start-3`}
          >
            {notesRow.node}
          </div>
        ) : null}
      </div>
      {advancedRow ? (
        <div
          data-actor-config-row="advanced"
          className="min-w-0 border-t border-[var(--glass-border-subtle)] pt-4"
        >
          {advancedRow.node}
        </div>
      ) : null}
    </div>
  );
}

interface ActorConfigAdvancedProps {
  title: string;
  /** Edits awaiting completion or save. */
  summary?: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: ReactNode;
}

/** Rarely needed settings, closed until asked for. Its content stays mounted to keep drafts. */
export function ActorConfigAdvanced({
  title,
  summary,
  open,
  onOpenChange,
  children,
}: ActorConfigAdvancedProps) {
  const panelId = useId();
  return (
    <div className="min-w-0">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => onOpenChange(!open)}
        className="flex min-h-[44px] w-full min-w-0 items-center gap-2 rounded-lg text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]/45"
      >
        <ChevronRightIcon
          size={16}
          aria-hidden="true"
          className={`shrink-0 text-[var(--color-text-muted)] transition-transform ${open ? "rotate-90" : ""}`}
        />
        <span className="shrink-0 text-sm font-semibold text-[var(--color-text-primary)]">
          {title}
        </span>
        {summary ? (
          <span data-changed className="min-w-0 truncate text-xs text-[var(--color-text-primary)]">
            {summary}
          </span>
        ) : null}
      </button>
      <div id={panelId} hidden={!open} className="pt-2">
        {children}
      </div>
    </div>
  );
}
