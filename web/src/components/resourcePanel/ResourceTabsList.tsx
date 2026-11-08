import { useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { FileText, Image as ImageGlyph, Presentation, RotateCcw } from "lucide-react";
import type { ClosedEntry, ResourceTab } from "../../stores/useResourcePanelStore";
import { resourceSource } from "./resourceSource";
import { classNames } from "../../utils/classNames";
import { CloseIcon, LcdIcon } from "../Icons";
import { buttonVariants } from "../ui/button-variants";

function matches(tab: ResourceTab, needle: string, source: string) {
  if (!needle) return true;
  return `${tab.title}\n${source}`.toLocaleLowerCase().includes(needle);
}

/**
 * The "all tabs" list: every open resource in tab order, a filter, and what was closed recently.
 * It never reorders the tabs; picking a row selects that tab.
 */
export function ResourceTabsList({
  tabs,
  activeKey,
  closed,
  onSelect,
  onClose,
  onRestore,
  onCloseOthers,
  onCloseAll,
  onDone,
}: {
  tabs: ResourceTab[];
  activeKey: string;
  closed: ClosedEntry[];
  onSelect: (tab: ResourceTab) => void;
  onClose: (tab: ResourceTab) => void;
  onRestore: (entry: ClosedEntry) => void;
  onCloseOthers?: () => void;
  onCloseAll?: () => void;
  onDone: () => void;
}) {
  const { t, i18n } = useTranslation("chat");
  const language = i18n.resolvedLanguage || i18n.language;
  const [query, setQuery] = useState("");
  const list = useRef<HTMLDivElement>(null);
  const needle = query.trim().toLocaleLowerCase();
  const shown = useMemo(
    () => tabs.filter((tab) => matches(tab, needle, resourceSource(tab, t, language))),
    [language, needle, t, tabs],
  );
  const shownClosed = useMemo(
    () =>
      closed.filter((entry) =>
        entry.tabs.some((tab) => matches(tab, needle, resourceSource(tab, t, language))),
      ),
    [closed, language, needle, t],
  );
  // Same-name tabs show where each came from; unique names keep the list quiet.
  const repeated = useMemo(() => {
    const seen = new Map<string, number>();
    for (const tab of tabs) seen.set(tab.title, (seen.get(tab.title) ?? 0) + 1);
    return seen;
  }, [tabs]);

  const move = (event: KeyboardEvent<HTMLElement>) => {
    const rows = [...(list.current?.querySelectorAll<HTMLElement>("[data-resource-row]") ?? [])];
    if (!rows.length) return;
    const index = rows.indexOf(document.activeElement as HTMLElement);
    if (event.key === "ArrowDown") {
      event.preventDefault();
      rows[(index + 1) % rows.length]?.focus();
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      rows[index <= 0 ? rows.length - 1 : index - 1]?.focus();
    }
  };

  return (
    <div
      className="flex max-h-[min(28rem,70vh)] w-[min(22rem,calc(100vw-2rem))] flex-col"
      onKeyDown={move}
    >
      <div className="shrink-0 p-2">
        <input
          autoFocus
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && shown[0]) {
              event.preventDefault();
              onSelect(shown[0]);
              onDone();
            }
          }}
          placeholder={t("resourcePanel.filter")}
          aria-label={t("resourcePanel.filter")}
          className="w-full rounded-md border border-[var(--glass-border-subtle)] bg-transparent px-2 py-1.5 text-sm outline-none focus:border-[var(--color-border-focus)]"
          data-resource-filter
        />
      </div>
      <div
        ref={list}
        className="min-h-0 flex-1 overflow-y-auto pb-1"
        role="listbox"
        aria-label={t("resourcePanel.tabs")}
      >
        {shown.map((tab) => {
          const source = resourceSource(tab, t, language);
          return (
            <div key={tab.key} className="group/row flex items-center pr-1">
              <button
                type="button"
                role="option"
                aria-selected={tab.key === activeKey}
                className={classNames(
                  "flex min-w-0 flex-1 items-center gap-2 px-3 py-1.5 text-left text-sm outline-none hover:bg-[var(--glass-tab-bg)] focus:bg-[var(--glass-tab-bg)]",
                  tab.key === activeKey && "font-semibold",
                )}
                onClick={() => {
                  onSelect(tab);
                  onDone();
                }}
                title={source}
                data-resource-row
              >
                <LcdIcon
                  icon={
                    tab.kind === "workspace"
                      ? FileText
                      : tab.kind === "presentation"
                        ? Presentation
                        : ImageGlyph
                  }
                  glyph={
                    tab.kind === "workspace"
                      ? "file-text"
                      : tab.kind === "presentation"
                        ? "monitor"
                        : "image"
                  }
                  className="h-4 w-4 shrink-0"
                  aria-hidden="true"
                />
                <span className="min-w-0 flex-1">
                  <span className="block truncate">{tab.title}</span>
                  {(repeated.get(tab.title) ?? 0) > 1 ? (
                    <span className="block truncate text-xs font-normal text-[var(--color-text-tertiary)]">
                      {source}
                    </span>
                  ) : null}
                </span>
              </button>
              <button
                type="button"
                className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded opacity-60 hover:bg-[var(--glass-tab-bg)] hover:opacity-100"
                aria-label={t("resourcePanel.closeTab", { name: tab.title })}
                title={t("resourcePanel.closeTab", { name: tab.title })}
                onClick={() => onClose(tab)}
              >
                <CloseIcon size={14} />
              </button>
            </div>
          );
        })}
        {!shown.length && tabs.length ? (
          <p className="px-3 py-2 text-sm text-[var(--color-text-tertiary)]">
            {t("resourcePanel.noMatch")}
          </p>
        ) : null}
        {shownClosed.length ? (
          <>
            <p className="mt-1 border-t border-[var(--glass-border-subtle)] px-3 pb-1 pt-2 text-xs text-[var(--color-text-tertiary)]">
              {t("resourcePanel.recentlyClosed")}
            </p>
            {shownClosed.map((entry) => (
              <button
                key={entry.id}
                type="button"
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm text-[var(--color-text-secondary)] outline-none hover:bg-[var(--glass-tab-bg)] focus:bg-[var(--glass-tab-bg)]"
                onClick={() => {
                  onRestore(entry);
                  onDone();
                }}
                title={t("resourcePanel.reopen")}
                data-resource-row
                data-resource-closed-entry
              >
                <LcdIcon
                  icon={RotateCcw}
                  glyph="undo"
                  className="h-4 w-4 shrink-0"
                  aria-hidden="true"
                />
                <span className="truncate">
                  {entry.tabs.length > 1
                    ? t("resourcePanel.closedGroup", {
                        title: entry.tabs[0].title,
                        rest: entry.tabs.length - 1,
                      })
                    : entry.tabs[0]?.title}
                </span>
              </button>
            ))}
          </>
        ) : null}
      </div>
      {tabs.length && (onCloseOthers || onCloseAll) ? (
        <div className="flex shrink-0 justify-end gap-1 border-t border-[var(--glass-border-subtle)] p-1.5">
          {onCloseOthers && tabs.length > 1 ? (
            <button
              type="button"
              className={`${buttonVariants({ variant: "ghost", size: "sm" })} h-8`}
              onClick={() => {
                onCloseOthers();
                onDone();
              }}
              data-resource-close-others
            >
              {t("resourcePanel.closeOthers")}
            </button>
          ) : null}
          {onCloseAll ? (
            <button
              type="button"
              className={`${buttonVariants({ variant: "ghost", size: "sm" })} h-8`}
              onClick={() => {
                onCloseAll();
                onDone();
              }}
              data-resource-close-all
            >
              {t("resourcePanel.closeAll")}
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
