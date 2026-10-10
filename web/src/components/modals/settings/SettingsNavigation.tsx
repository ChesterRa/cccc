import { useActiveSettingsTab } from "./useActiveSettingsTab";
import { useTranslation } from "react-i18next";
import { InfoIcon } from "../../Icons";
import { ScrollFade } from "../../ScrollFade";
import type { SettingsScope } from "./types";
import { ScopeTooltip } from "./ScopeTooltip";

interface SettingsTabOption {
  id: string;
  label: string;
}

interface SettingsNavigationProps {
  isDark: boolean;
  groupId?: string;
  groupTitle?: string;
  scope: SettingsScope;
  scopeRootUrl: string;
  globalEnabled: boolean;
  tabs: SettingsTabOption[];
  activeTab: string;
  onScopeChange: (scope: SettingsScope) => void;
  onTabChange: (tabId: string) => void;
}

export function SettingsNavigation({
  isDark,
  groupId,
  groupTitle,
  scope,
  scopeRootUrl,
  globalEnabled,
  tabs,
  activeTab,
  onScopeChange,
  onTabChange,
}: SettingsNavigationProps) {
  const { t } = useTranslation("settings");
  const activeMobileTabRef = useActiveSettingsTab();
  const globalScopeTitle = globalEnabled
    ? t("navigation.globalScopeTitle")
    : t("navigation.globalLockedTitle");
  const globalScopeContent = globalEnabled
    ? t("navigation.globalScopeContent")
    : t("navigation.globalLockedContent");
  const tabButtonClass = (active: boolean) =>
    `w-full flex h-9 items-center rounded-lg px-3 text-sm transition-[background-color,color,box-shadow] ${
      active
        ? "bg-[var(--color-bg-primary)] font-semibold text-[var(--color-text-primary)] shadow-[0_0_0_1px_var(--glass-border-subtle)]"
        : "text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]"
    }`;
  const mobileScopeButtonClass = (active: boolean) =>
    `min-w-0 flex-1 relative flex items-center justify-center px-3 py-2.5 rounded-xl text-sm min-h-[44px] font-medium transition-[background-color,border-color,color,box-shadow] ${
      active
        ? "border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg-active)] text-[var(--color-text-primary)] shadow-sm"
        : "border border-transparent bg-transparent text-[var(--color-text-tertiary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]"
    }`;

  return (
    <>
      <aside className="hidden min-h-0 w-64 shrink-0 flex-col border-r border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] sm:flex">
        <div className="flex flex-col gap-3 px-3 pt-4 pb-2">
          <div
            role="group"
            aria-label={t("navigation.targetScope")}
            className="grid grid-cols-2 rounded-[10px] bg-[var(--glass-tab-bg)] p-[3px]"
            data-settings-scope-switch
          >
            {(
              [
                ["group", t("navigation.thisGroup"), !groupId],
                ["global", t("navigation.global"), !globalEnabled],
              ] as const
            ).map(([value, label, disabled]) => (
              <button
                key={value}
                type="button"
                aria-pressed={scope === value}
                disabled={disabled}
                onClick={() => onScopeChange(value)}
                className={`h-8 min-w-0 truncate rounded-lg px-2 text-[13px] transition-colors focus-visible:outline-2 disabled:opacity-40 ${
                  scope === value
                    ? "bg-[var(--color-bg-primary)] font-semibold text-[var(--color-text-primary)] shadow-[0_1px_2px_rgba(24,24,27,0.08)]"
                    : "text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
                }`}
              >
                {label}
              </button>
            ))}
          </div>
          <div className="flex items-center gap-1 px-2 text-xs text-[var(--color-text-muted)]">
            <span className="min-w-0 truncate">
              {scope === "group"
                ? t("navigation.appliesToGroup", {
                    name: groupTitle || scopeRootUrl || groupId || "—",
                  })
                : t("navigation.appliesToGlobal")}
            </span>
            <ScopeTooltip
              isDark={isDark}
              title={scope === "group" ? t("navigation.groupScopeTitle") : globalScopeTitle}
              content={
                scope === "group" ? (
                  <>{t("navigation.groupScopeContent", { scopeRoot: scopeRootUrl || groupId })}</>
                ) : (
                  <>{globalScopeContent}</>
                )
              }
            >
              {(getReferenceProps, setReference) => (
                <span
                  ref={setReference}
                  {...getReferenceProps()}
                  className="shrink-0 rounded-full p-1 opacity-60 hover:bg-black/5 dark:hover:bg-white/5"
                >
                  <InfoIcon size={12} />
                </span>
              )}
            </ScopeTooltip>
          </div>
        </div>

        <nav className="flex-1 overflow-y-auto scrollbar-hide px-3 pb-4 pt-2">
          <div className="space-y-1">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                onClick={() => onTabChange(tab.id)}
                aria-current={activeTab === tab.id ? "page" : undefined}
                className={tabButtonClass(activeTab === tab.id)}
              >
                {tab.label}
              </button>
            ))}
          </div>
        </nav>
      </aside>

      <div className="sm:hidden flex flex-col flex-shrink-0">
        <div className="px-4 py-3 border-b border-[var(--glass-border-subtle)]">
          <div className="flex items-stretch gap-2">
            <button
              type="button"
              onClick={() => onScopeChange("group")}
              aria-pressed={scope === "group"}
              disabled={!groupId}
              className={`${mobileScopeButtonClass(scope === "group")} disabled:opacity-40`}
            >
              <span className="min-w-0 pr-2">
                <span className="block">{t("navigation.thisGroup")}</span>
                {groupTitle && (
                  <span className="block truncate text-xs font-normal text-[var(--color-text-secondary)]">
                    {groupTitle}
                  </span>
                )}
              </span>
              <div className="absolute right-1 top-1/2 -translate-y-1/2">
                <ScopeTooltip
                  isDark={isDark}
                  title={t("navigation.groupScopeTitle")}
                  content={
                    <>{t("navigation.groupScopeContent", { scopeRoot: scopeRootUrl || groupId })}</>
                  }
                >
                  {(getReferenceProps, setReference) => (
                    <div
                      ref={setReference}
                      {...getReferenceProps({ onClick: (e) => e.stopPropagation() })}
                      className="p-1.5 hover:bg-black/5 dark:hover:bg-white/5 rounded-full transition-colors opacity-50"
                    >
                      <InfoIcon size={14} />
                    </div>
                  )}
                </ScopeTooltip>
              </div>
            </button>

            <button
              type="button"
              onClick={() => onScopeChange("global")}
              aria-pressed={scope === "global"}
              disabled={!globalEnabled}
              className={`${mobileScopeButtonClass(scope === "global")} disabled:opacity-40`}
            >
              <span className="min-w-0 pr-2">{t("navigation.global")}</span>
              <div className="absolute right-1 top-1/2 -translate-y-1/2">
                <ScopeTooltip
                  isDark={isDark}
                  title={globalScopeTitle}
                  content={<>{globalScopeContent}</>}
                >
                  {(getReferenceProps, setReference) => (
                    <div
                      ref={setReference}
                      {...getReferenceProps({ onClick: (e) => e.stopPropagation() })}
                      className="p-1.5 hover:bg-black/5 dark:hover:bg-white/5 rounded-full transition-colors opacity-50"
                    >
                      <InfoIcon size={14} />
                    </div>
                  )}
                </ScopeTooltip>
              </div>
            </button>
          </div>
        </div>

        <ScrollFade
          className="flex-shrink-0 w-full border-b border-[var(--glass-border-subtle)]"
          innerClassName="flex min-h-[54px] px-4 py-2"
          fadeWidth={20}
        >
          {tabs.map((tab) => (
            <button
              key={tab.id}
              ref={activeTab === tab.id ? activeMobileTabRef : undefined}
              aria-current={activeTab === tab.id ? "page" : undefined}
              onClick={() => onTabChange(tab.id)}
              className={`min-h-10 shrink-0 rounded-lg border px-3 py-2 text-xs font-medium whitespace-nowrap ${
                activeTab === tab.id
                  ? "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg-active)] text-[var(--color-text-primary)]"
                  : "border-transparent text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]"
              }`}
            >
              {tab.label}
            </button>
          ))}
        </ScrollFade>
      </div>
    </>
  );
}
