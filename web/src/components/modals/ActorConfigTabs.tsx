import { type KeyboardEvent, type ReactNode, useId } from "react";
import { SettingsIcon } from "../Icons";
import { nextActorConfigTabId } from "./actorConfigTabsModel";

export interface ActorConfigTab {
  id: string;
  label: string;
  panel: ReactNode;
  disabledReason?: string;
  panelId?: string;
  /** A secondary view: set apart at the end of the strip instead of among its siblings. */
  aside?: boolean;
  /** Something behind this tab that needs noticing, such as changed settings. */
  attention?: string;
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

  const renderedPanels = (() => {
    const panels: { id: string; panel: ReactNode; selected: boolean; labelledBy: string }[] = [];
    const seenPanelKeys = new Set<string>();

    for (const tab of tabs) {
      const panelKey = tab.panelId ?? tab.id;
      if (seenPanelKeys.has(panelKey)) continue;
      seenPanelKeys.add(panelKey);

      const matchingTabs = tabs.filter((t) => (t.panelId ?? t.id) === panelKey);
      const isSelected = matchingTabs.some((t) => t.id === activeTab.id);
      const activeMatchingTab = matchingTabs.find((t) => t.id === activeTab.id) ?? matchingTabs[0];

      panels.push({
        id: panelKey,
        panel: activeMatchingTab.panel,
        selected: isSelected,
        labelledBy: `${baseId}-tab-${activeMatchingTab.id}`,
      });
    }
    return panels;
  })();

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
                aria-controls={`${baseId}-panel-${tab.panelId ?? tab.id}`}
                tabIndex={selected ? 0 : -1}
                data-aside={tab.aside || undefined}
                className={`${tab.aside ? "ml-auto inline-flex items-center gap-1.5 border-l border-l-[var(--glass-border-subtle)] pl-4 " : ""}shrink-0 border-b-2 px-3 py-2 text-xs font-medium transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]/45 ${selected ? "border-[var(--color-accent-primary)] text-[var(--color-text-primary)] dark:border-white dark:text-white dark:shadow-[0_1px_8px_rgba(255,255,255,0.18)]" : "border-transparent text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)] dark:hover:text-zinc-200"}`}
                onClick={tab.disabledReason ? undefined : () => onChange(tab.id)}
                onKeyDown={tab.disabledReason ? undefined : handleKeyDown}
              >
                {tab.aside ? <SettingsIcon size={13} aria-hidden="true" /> : null}
                {tab.label}
                {tab.attention ? (
                  <>
                    <span
                      aria-hidden="true"
                      title={tab.attention}
                      className="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-500"
                    />
                    <span className="sr-only">{tab.attention}</span>
                  </>
                ) : null}
              </button>
            );
          })}
        </div>
      </div>
      {renderedPanels.map((panel) => (
        <div
          key={panel.id}
          id={`${baseId}-panel-${panel.id}`}
          role="tabpanel"
          aria-labelledby={panel.labelledBy}
          className={
            fill && panel.selected ? "flex min-h-0 min-w-0 flex-1 flex-col" : "min-w-0 pt-4"
          }
          hidden={!panel.selected}
        >
          {panel.panel}
        </div>
      ))}
    </div>
  );
}
