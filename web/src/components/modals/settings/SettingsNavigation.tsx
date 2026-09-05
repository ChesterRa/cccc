import { useActiveSettingsTab } from "./useActiveSettingsTab";
import { useTranslation } from "react-i18next";
import { InfoIcon } from "../../Icons";
import { ScrollFade } from "../../ScrollFade";
import type { SettingsScope } from "./types";
import { ScopeTooltip } from "./ScopeTooltip";
import { settingsWorkspaceSoftPanelClass } from "./types";

interface SettingsTabOption {
  id: string;
  label: string;
}

interface SettingsNavigationProps {
  isDark: boolean;
  groupId?: string;
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
  scope,
  scopeRootUrl,
  globalEnabled,
  tabs,
  activeTab,
  onScopeChange,
  onTabChange,
}: SettingsNavigationProps) {
  const { t } = useTranslation("settings");
  const activeMobileTabRef = useActiveSettingsTab(scope, activeTab);
  const globalScopeTitle = globalEnabled
    ? t("navigation.globalScopeTitle")
    : t("navigation.globalLockedTitle");
  const globalScopeContent = globalEnabled
    ? t("navigation.globalScopeContent")
    : t("navigation.globalLockedContent");
  const scopeButtonClass = (active: boolean) =>
    `w-full flex items-center justify-between rounded-[16px] border px-3.5 py-2.5 text-left text-sm font-semibold transition-[background-color,border-color,color,box-shadow] ${
      active
        ? "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg-active)] text-[var(--color-text-primary)] shadow-[0_10px_30px_rgba(15,23,42,0.06)]"
        : "border-transparent bg-transparent text-[var(--color-text-tertiary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]"
    }`;
  const tabButtonClass = (active: boolean) =>
    `w-full flex items-center rounded-[14px] px-3 py-2.5 text-sm font-medium transition-[background-color,border-color,color,box-shadow] ${
      active
        ? "border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg-active)] text-[var(--color-text-primary)] shadow-[0_8px_22px_rgba(15,23,42,0.05)]"
        : "border border-transparent text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--glass-tab-bg-hover)]"
    }`;
  const mobileScopeButtonClass = (active: boolean) =>
    `flex-1 relative flex items-center justify-center px-3 py-2.5 rounded-xl text-sm min-h-[44px] font-medium transition-[background-color,border-color,color,box-shadow] ${
      active
        ? "border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg-active)] text-[var(--color-text-primary)] shadow-sm"
        : "border border-transparent bg-transparent text-[var(--color-text-tertiary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]"
    }`;

  return (
    <>
      <aside
        className={`hidden border-r border-[var(--glass-border-subtle)] sm:flex sm:w-60 lg:w-[16.5rem] sm:flex-col shrink-0 ${
          isDark
            ? "bg-[linear-gradient(180deg,rgba(20,22,26,0.96),rgba(14,15,18,0.92))]"
            : "bg-[linear-gradient(180deg,rgba(255,255,255,0.99),rgba(247,249,252,0.94))]"
        }`}
      >
        <div className="border-b border-[var(--glass-border-subtle)] px-4 pb-3 pt-4 lg:px-4">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[var(--color-text-muted)]">
            {t("navigation.targetScope")}
          </div>
          <div className="mt-3 flex flex-col gap-2">
            <button
              type="button"
              onClick={() => onScopeChange("group")}
              disabled={!groupId}
              className={`${scopeButtonClass(scope === "group")} disabled:opacity-40`}
            >
              <div className="min-w-0">
                <div>{t("navigation.thisGroup")}</div>
                <div className="mt-0.5 truncate text-[11px] font-medium text-[var(--color-text-muted)]">
                  {scopeRootUrl || groupId || "—"}
                </div>
              </div>
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
                    className="p-1 -mr-1 hover:bg-black/5 dark:hover:bg-white/5 rounded-full transition-colors opacity-50"
                  >
                    <InfoIcon size={12} />
                  </div>
                )}
              </ScopeTooltip>
            </button>

            <button
              type="button"
              onClick={() => onScopeChange("global")}
              disabled={!globalEnabled}
              className={`${scopeButtonClass(scope === "global")} disabled:opacity-40`}
            >
              <div className="min-w-0">
                <div>{t("navigation.global")}</div>
                <div className="mt-0.5 text-[11px] font-medium text-[var(--color-text-muted)]">
                  {globalEnabled ? globalScopeTitle : t("navigation.globalLockedTitle")}
                </div>
              </div>
              <ScopeTooltip
                isDark={isDark}
                title={globalScopeTitle}
                content={<>{globalScopeContent}</>}
              >
                {(getReferenceProps, setReference) => (
                  <div
                    ref={setReference}
                    {...getReferenceProps({ onClick: (e) => e.stopPropagation() })}
                    className="p-1 -mr-1 hover:bg-black/5 dark:hover:bg-white/5 rounded-full transition-colors opacity-50"
                  >
                    <InfoIcon size={12} />
                  </div>
                )}
              </ScopeTooltip>
            </button>
          </div>
        </div>

        <nav className="flex-1 overflow-y-auto scrollbar-hide px-4 pb-4 pt-3 lg:px-4">
          <div className="px-2 pb-2 text-[11px] font-semibold uppercase tracking-[0.16em] text-[var(--color-text-muted)]">
            {t("navigation.sections", { defaultValue: "Sections" })}
          </div>
          <div className="space-y-1">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                onClick={() => onTabChange(tab.id)}
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
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => onScopeChange("group")}
              disabled={!groupId}
              className={`${mobileScopeButtonClass(scope === "group")} disabled:opacity-40`}
            >
              <span>{t("navigation.thisGroup")}</span>
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
              disabled={!globalEnabled}
              className={`${mobileScopeButtonClass(scope === "global")} disabled:opacity-40`}
            >
              <span>{t("navigation.global")}</span>
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
              className={`${settingsWorkspaceSoftPanelClass(isDark)} flex-shrink-0 px-4 py-2.5 text-xs font-medium whitespace-nowrap ${
                activeTab === tab.id
                  ? "!border-[var(--glass-border-subtle)] !bg-[var(--glass-tab-bg-active)] text-[var(--color-text-primary)]"
                  : "text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
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
