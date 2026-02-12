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
  scope: SettingsScope;
  scopeRootUrl: string;
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
  tabs,
  activeTab,
  onScopeChange,
  onTabChange,
}: SettingsNavigationProps) {
  return (
    <>
      <aside className={`hidden sm:flex sm:flex-col w-48 border-r flex-shrink-0 ${isDark ? "bg-slate-900/50 border-slate-800" : "bg-gray-50/50 border-gray-100"}`}>
        <div className="p-3 space-y-3">
          <div className={`px-3 text-[10px] font-bold uppercase tracking-wider opacity-30 ${isDark ? "text-slate-400" : "text-gray-500"}`}>
            Target Scope
          </div>
          <div className="flex flex-col gap-1">
            <button
              type="button"
              onClick={() => onScopeChange("group")}
              disabled={!groupId}
              className={`w-full flex items-center justify-between px-3 py-2 rounded-lg text-sm text-left font-semibold transition-colors ${
                scope === "group"
                  ? isDark
                    ? "bg-emerald-500/15 text-emerald-300 border border-emerald-500/30"
                    : "bg-emerald-50 text-emerald-700 border border-emerald-200"
                  : isDark
                    ? "hover:bg-slate-800 text-slate-400"
                    : "hover:bg-gray-100 text-gray-600"
              } disabled:opacity-40`}
            >
              <span>This group</span>
              <ScopeTooltip
                isDark={isDark}
                title="Group Scope"
                content={
                  <>
                    Applies to <span className="font-mono text-emerald-500">{scopeRootUrl || groupId}</span> only. Useful for group-specific timeouts and integrations.
                  </>
                }
              >
                {(getReferenceProps, setReference) => (
                  <div
                    ref={setReference}
                    {...getReferenceProps({
                      onClick: (e) => e.stopPropagation(),
                    })}
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
              className={`w-full flex items-center justify-between px-3 py-2 rounded-lg text-sm text-left font-semibold transition-colors ${
                scope === "global"
                  ? isDark
                    ? "bg-emerald-500/15 text-emerald-300 border border-emerald-500/30"
                    : "bg-emerald-50 text-emerald-700 border border-emerald-200"
                  : isDark
                    ? "hover:bg-slate-800 text-slate-400"
                    : "hover:bg-gray-100 text-gray-600"
              }`}
            >
              <span>Global</span>
              <ScopeTooltip
                isDark={isDark}
                title="Global Scope"
                content={<>Applies to your whole CCCC instance (daemon + Web). These settings affect all groups unless overridden.</>}
              >
                {(getReferenceProps, setReference) => (
                  <div
                    ref={setReference}
                    {...getReferenceProps({
                      onClick: (e) => e.stopPropagation(),
                    })}
                    className="p-1 -mr-1 hover:bg-black/5 dark:hover:bg-white/5 rounded-full transition-colors opacity-50"
                  >
                    <InfoIcon size={12} />
                  </div>
                )}
              </ScopeTooltip>
            </button>
          </div>
        </div>

        <div className={`mx-3 border-b ${isDark ? "border-slate-800" : "border-gray-100"}`} />

        <nav className="flex-1 overflow-y-auto p-3 space-y-1">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => onTabChange(tab.id)}
              className={`w-full flex items-center px-3 py-2 text-xs font-medium rounded-lg transition-colors ${
                activeTab === tab.id
                  ? isDark
                    ? "bg-slate-800 text-emerald-400"
                    : "bg-white shadow-sm border border-gray-200 text-emerald-600"
                  : isDark
                    ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800/50"
                    : "text-gray-500 hover:text-gray-700 hover:bg-gray-100"
              }`}
            >
              {tab.label}
            </button>
          ))}
        </nav>
      </aside>

      <div className="sm:hidden flex flex-col flex-shrink-0">
        <div className={`px-5 py-3 border-b ${isDark ? "border-slate-800" : "border-gray-200"}`}>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => onScopeChange("group")}
              disabled={!groupId}
              className={`flex-1 relative flex items-center justify-center px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                scope === "group"
                  ? isDark
                    ? "bg-emerald-500/15 text-emerald-300 border border-emerald-500/30"
                    : "bg-emerald-50 text-emerald-700 border border-emerald-200"
                  : isDark
                    ? "bg-slate-900 text-slate-300 border border-slate-800 hover:bg-slate-800"
                    : "bg-white text-gray-700 border border-gray-200 hover:bg-gray-50"
              } disabled:opacity-40`}
            >
              <span>This group</span>
              <div className="absolute right-1 top-1/2 -translate-y-1/2">
                <ScopeTooltip
                  isDark={isDark}
                  title="Group Scope"
                  content={
                    <>
                      Applies to <span className="font-mono text-emerald-500">{scopeRootUrl || groupId}</span> only. Useful for group-specific timeouts and integrations.
                    </>
                  }
                >
                  {(getReferenceProps, setReference) => (
                    <div
                      ref={setReference}
                      {...getReferenceProps({
                        onClick: (e) => e.stopPropagation(),
                      })}
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
              className={`flex-1 relative flex items-center justify-center px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                scope === "global"
                  ? isDark
                    ? "bg-emerald-500/15 text-emerald-300 border border-emerald-500/30"
                    : "bg-emerald-50 text-emerald-700 border border-emerald-200"
                  : isDark
                    ? "bg-slate-900 text-slate-300 border border-slate-800 hover:bg-slate-800"
                    : "bg-white text-gray-700 border border-gray-200 hover:bg-gray-50"
              }`}
            >
              <span>Global</span>
              <div className="absolute right-1 top-1/2 -translate-y-1/2">
                <ScopeTooltip
                  isDark={isDark}
                  title="Global Scope"
                  content={<>Applies to your whole CCCC instance (daemon + Web). These settings affect all groups unless overridden.</>}
                >
                  {(getReferenceProps, setReference) => (
                    <div
                      ref={setReference}
                      {...getReferenceProps({
                        onClick: (e) => e.stopPropagation(),
                      })}
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
          className={`flex-shrink-0 w-full border-b ${isDark ? "border-slate-800" : "border-gray-200"}`}
          innerClassName="flex min-h-[48px]"
          fadeWidth={20}
        >
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => onTabChange(tab.id)}
              className={`flex-shrink-0 px-4 py-2.5 text-xs font-medium transition-colors whitespace-nowrap ${
                activeTab === tab.id
                  ? isDark
                    ? "text-emerald-400 border-b-2 border-emerald-400"
                    : "text-emerald-600 border-b-2 border-emerald-600"
                  : isDark
                    ? "text-slate-400 hover:text-slate-200"
                    : "text-gray-500 hover:text-gray-700"
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
