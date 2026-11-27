import { type ReactNode, useId } from "react";
import { ChevronRightIcon } from "../Icons";

interface ActorConfigFormProps {
  identityLabel: string;
  runtimeLabel: string;
  /** Who the Agent is: avatar, name, role. */
  identity: ReactNode;
  /** How the Agent runs. */
  runtime: ReactNode;
  /** Full-width rows under the two columns; an absent row leaves no divider behind. */
  directory?: ReactNode;
  notes?: ReactNode;
  advanced?: ReactNode;
}

/**
 * The Agent form as one flat sheet: identity and runtime side by side on wide screens, then
 * full-width rows separated by hairlines. Narrow screens stack the same order.
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
  return (
    <div className="flex min-w-0 flex-1 flex-col divide-y divide-[var(--glass-border-subtle)]">
      <div className="grid grid-cols-1 gap-x-8 gap-y-5 pb-5 lg:grid-cols-2">
        <section aria-label={identityLabel} className="min-w-0">
          {identity}
        </section>
        <section aria-label={runtimeLabel} className="min-w-0">
          {runtime}
        </section>
      </div>
      {rows.map((row) => (
        <div
          key={row.id}
          data-actor-config-row={row.id}
          // The notes take whatever height the form has left; every other row keeps its own.
          className={`min-w-0 py-4 last:pb-0 ${row.id === "notes" ? "flex flex-1 flex-col" : ""}`}
        >
          {row.node}
        </div>
      ))}
    </div>
  );
}

interface ActorConfigAdvancedProps {
  title: string;
  /** What the closed section holds, or what was changed inside it. */
  summary: string;
  /** True when `summary` reports settings the user changed, so it must not read as a hint. */
  changed: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: ReactNode;
}

/** Rarely needed settings, closed until asked for. Its content stays mounted to keep drafts. */
export function ActorConfigAdvanced({
  title,
  summary,
  changed,
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
        {open && !changed ? null : (
          <span
            data-changed={changed || undefined}
            className={`min-w-0 truncate text-xs ${changed ? "text-[var(--color-text-primary)]" : "text-[var(--color-text-muted)]"}`}
          >
            {summary}
          </span>
        )}
      </button>
      <div id={panelId} hidden={!open} className="pt-2">
        {children}
      </div>
    </div>
  );
}

interface ActorConfigViewsProps {
  showAdvanced: boolean;
  form: ReactNode;
  advanced?: ReactNode;
}

/**
 * The main form and its advanced settings share one area and only one is shown. Both stay
 * mounted, so drafts survive a switch. Switching cross-fades with a short slide.
 */
export function ActorConfigViews({ showAdvanced, form, advanced }: ActorConfigViewsProps) {
  const hasAdvanced = advanced != null && advanced !== false;
  const advancedShown = showAdvanced && hasAdvanced;
  // Visibility is part of the transition, so the outgoing view fades and slides out before it
  // stops being painted, and the incoming one is visible from the first frame. The view that
  // is not shown is taken out of flow, so only the visible one decides how far the area scrolls.
  const cell =
    "flex min-w-0 flex-col transition-[opacity,transform,visibility] duration-200 ease-out motion-reduce:transition-none";
  const shown = "visible flex-1 translate-x-0 opacity-100";
  const away = "invisible absolute inset-0 overflow-hidden opacity-0";
  return (
    <div className="relative flex min-w-0 flex-1 flex-col overflow-x-clip">
      <div
        data-actor-config-view="form"
        inert={advancedShown}
        className={`${cell} ${advancedShown ? `${away} -translate-x-3` : shown}`}
      >
        {form}
      </div>
      {hasAdvanced ? (
        <div
          data-actor-config-view="advanced"
          inert={!advancedShown}
          className={`${cell} ${advancedShown ? shown : `${away} translate-x-3`}`}
        >
          {advanced}
        </div>
      ) : null}
    </div>
  );
}
