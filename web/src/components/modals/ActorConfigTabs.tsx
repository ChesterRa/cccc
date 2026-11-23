import { type KeyboardEvent, type ReactNode, useId } from "react";
import { nextActorConfigTabId } from "./actorConfigTabsModel";

export interface ActorConfigTab {
  id: string;
  label: string;
  panel: ReactNode;
  disabledReason?: string;
}

interface ActorConfigTabsProps {
  ariaLabel: string;
  tabs: readonly ActorConfigTab[];
  activeId: string;
  onChange: (id: string) => void;
  fill?: boolean;
  hideTabs?: boolean;
}

export function ActorConfigTabs({
  ariaLabel,
  tabs,
  activeId,
  onChange,
  fill,
  hideTabs,
}: ActorConfigTabsProps) {
  const baseId = useId();
  const available = tabs.filter((tab) => !tab.disabledReason);
  const activeTab = available.find((tab) => tab.id === activeId) ?? available[0];

  if (!activeTab) return null;

  const focusTab = (id: string) => {
    requestAnimationFrame(() => {
      const tab = document.getElementById(`${baseId}-tab-${id}`);
      tab?.focus();
      tab?.scrollIntoView({ block: "nearest", inline: "nearest" });
    });
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    const nextId = nextActorConfigTabId(
      available.map((tab) => tab.id),
      activeTab.id,
      event.key,
    );
    if (nextId === activeTab.id || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key))
      return;
    event.preventDefault();
    onChange(nextId);
    focusTab(nextId);
  };

  return (
    <div className={fill ? "flex min-h-0 min-w-0 flex-1 flex-col" : "min-w-0"}>
      <div hidden={hideTabs} className="shrink-0 overflow-x-auto scrollbar-hide">
        <div
          role="tablist"
          aria-label={ariaLabel}
          className="inline-flex min-w-full items-center gap-1 border-b border-[var(--glass-border-subtle)]"
        >
          {tabs.map((tab) => {
            const selected = tab.id === activeTab.id;
            return (
              <button
                key={tab.id}
                id={`${baseId}-tab-${tab.id}`}
                type="button"
                role="tab"
                aria-selected={selected}
                aria-disabled={tab.disabledReason ? true : undefined}
                title={tab.disabledReason}
                aria-controls={`${baseId}-panel-${tab.id}`}
                tabIndex={selected ? 0 : -1}
                className={`shrink-0 border-b-2 px-3 py-2 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]/45 ${selected ? "border-[var(--color-accent-primary)] text-[var(--color-text-primary)]" : "border-transparent text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]"}`}
                onClick={tab.disabledReason ? undefined : () => onChange(tab.id)}
                onKeyDown={tab.disabledReason ? undefined : handleKeyDown}
              >
                {tab.label}
              </button>
            );
          })}
        </div>
      </div>
      {tabs.map((tab) => {
        const selected = tab.id === activeTab.id;
        return (
          <div
            key={tab.id}
            id={`${baseId}-panel-${tab.id}`}
            role="tabpanel"
            aria-labelledby={`${baseId}-tab-${tab.id}`}
            className={fill && selected ? "flex min-h-0 min-w-0 flex-1 flex-col" : "min-w-0 pt-4"}
            hidden={!selected}
          >
            {tab.panel}
          </div>
        );
      })}
    </div>
  );
}
