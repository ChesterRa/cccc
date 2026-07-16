import { type KeyboardEvent, type ReactNode, useId } from "react";
import { nextActorConfigTabId } from "./actorConfigTabsModel";

export interface ActorConfigTab {
  id: string;
  label: string;
  panel: ReactNode;
}

interface ActorConfigTabsProps {
  ariaLabel: string;
  tabs: readonly ActorConfigTab[];
  activeId: string;
  onChange: (id: string) => void;
}

export function ActorConfigTabs({ ariaLabel, tabs, activeId, onChange }: ActorConfigTabsProps) {
  const baseId = useId();
  const activeTab = tabs.find((tab) => tab.id === activeId) ?? tabs[0];

  if (!activeTab) return null;

  const focusTab = (id: string) => {
    requestAnimationFrame(() => document.getElementById(`${baseId}-tab-${id}`)?.focus());
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    const nextId = nextActorConfigTabId(
      tabs.map((tab) => tab.id),
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
    <div className="min-w-0">
      <div className="overflow-x-auto scrollbar-hide">
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
                aria-controls={`${baseId}-panel-${tab.id}`}
                tabIndex={selected ? 0 : -1}
                className={`shrink-0 border-b-2 px-3 py-2 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]/45 ${selected ? "border-[var(--color-accent-primary)] text-[var(--color-text-primary)]" : "border-transparent text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]"}`}
                onClick={() => onChange(tab.id)}
                onKeyDown={handleKeyDown}
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
            className="min-w-0 pt-4"
            hidden={!selected}
          >
            {tab.panel}
          </div>
        );
      })}
    </div>
  );
}
