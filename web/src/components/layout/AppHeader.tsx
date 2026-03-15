import { useTranslation } from 'react-i18next';
import { Actor, GroupDoc, Theme } from "../../types";
import { getGroupStatusUnified } from "../../utils/groupStatus";
import { classNames } from "../../utils/classNames";
import { ThemeToggleCompact } from "../ThemeToggle";
import { LanguageSwitcher } from "../LanguageSwitcher";
import {
  ClipboardIcon,
  SearchIcon,
  RocketIcon,
  PlayIcon,
  PauseIcon,
  StopIcon,
  SettingsIcon,
  EditIcon,
  MoreIcon,
  MenuIcon
} from "../Icons";

export interface AppHeaderProps {
  isDark: boolean;
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  webReadOnly?: boolean;
  webVersion?: string;
  daemonVersion?: string;
  selectedGroupId: string;
  groupDoc: GroupDoc | null;
  selectedGroupRunning: boolean;
  actors: Actor[];
  sseStatus: "connected" | "connecting" | "disconnected";
  busy: string;
  onOpenSidebar: () => void;
  onOpenGroupEdit?: () => void;
  onOpenSearch: () => void;
  onOpenContext: () => void;
  onStartGroup: () => void;
  onStopGroup: () => void;
  onSetGroupState: (state: "active" | "paused" | "idle") => void | Promise<void>;
  onOpenSettings: () => void;
  onOpenMobileMenu: () => void;
}

export function AppHeader({
  isDark,
  theme,
  onThemeChange,
  webReadOnly,
  webVersion,
  daemonVersion,
  selectedGroupId,
  groupDoc,
  selectedGroupRunning,
  actors,
  busy,
  onOpenSidebar,
  onOpenGroupEdit,
  onOpenSearch,
  onOpenContext,
  onStartGroup,
  onStopGroup,
  onSetGroupState,
  onOpenSettings,
  onOpenMobileMenu,
  sseStatus,
}: AppHeaderProps) {
  const { t } = useTranslation('layout');
  const headerIconButtonClass =
    "flex items-center justify-center w-11 h-11 rounded-xl transition-all shrink-0 glass-btn";
  return (
    <header
      className="flex-shrink-0 z-20 px-4 h-14 flex items-center justify-between gap-3 glass-header"
    >
      <div className="flex items-center gap-3 min-w-0">
        <button
          className={classNames(
            "md:hidden -ml-1",
            headerIconButtonClass,
            "text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)]"
          )}
          onClick={onOpenSidebar}
          aria-label={t('openSidebar')}
        >
          <MenuIcon size={18} />
        </button>

        <div className="min-w-0 flex flex-col">
          <div className="flex items-center gap-2">
            <h1 className="text-sm font-semibold truncate text-[var(--color-text-primary)]">
              {groupDoc?.title || (selectedGroupId ? selectedGroupId : t('selectGroup'))}
            </h1>
            {selectedGroupId && sseStatus !== "connected" && (
              <span
                className={classNames(
                  "flex-shrink-0 w-2 h-2 rounded-full",
                  sseStatus === "connecting" ? "bg-amber-400 animate-pulse" : "bg-rose-500"
                )}
                title={sseStatus === "connecting" ? t('reconnecting') : t('disconnected')}
              />
            )}
            {selectedGroupId &&
              groupDoc &&
              (() => {
                const status = getGroupStatusUnified(selectedGroupRunning, groupDoc.state);
                return (
                  <span
                    className={classNames(
                      "w-2 h-2 rounded-full ring-2",
                      status.dotClass,
                      "ring-black/10 dark:ring-white/10"
                    )}
                    title={status.label}
                  />
                );
              })()}
          </div>
          {(webVersion || daemonVersion) && (
            <div className="mt-0.5 flex flex-wrap items-center gap-1.5 text-[11px] text-[var(--color-text-tertiary)]">
              {webVersion && (
                <span
                  className="inline-flex items-center rounded-full border border-black/10 bg-black/5 px-2 py-0.5 font-mono dark:border-white/10 dark:bg-white/5"
                  title={t("connectedWebVersionTitle", { version: webVersion })}
                >
                  {t("connectedWebVersion", { version: webVersion })}
                </span>
              )}
              {daemonVersion && (
                <span
                  className="inline-flex items-center rounded-full border border-black/10 bg-black/5 px-2 py-0.5 font-mono dark:border-white/10 dark:bg-white/5"
                  title={t("connectedDaemonVersionTitle", { version: daemonVersion })}
                >
                  {t("connectedDaemonVersion", { version: daemonVersion })}
                </span>
              )}
            </div>
          )}
        </div>

        {selectedGroupId && !webReadOnly && onOpenGroupEdit && (
          <button
            className={classNames(
              "hidden md:inline-flex items-center justify-center gap-1 text-xs px-2.5 py-1.5 rounded-xl transition-all glass-btn",
              "text-[var(--color-text-secondary)]"
            )}
            onClick={onOpenGroupEdit}
            title={t('editGroup')}
            aria-label={t('editGroup')}
          >
            <EditIcon size={14} />
          </button>
        )}
      </div>

      {/* Right Actions */}
      <div className="flex items-center gap-1">
        {!webReadOnly && (
          <>
            {/* Desktop Actions */}
            <div className="hidden md:flex items-center gap-1.5 mr-2">
              <button
                onClick={onOpenSearch}
                disabled={!selectedGroupId}
                className={classNames(
                  headerIconButtonClass,
                  "text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)]"
                )}
                title={t('searchMessages')}
              >
                <span className="sr-only">{t('searchMessages')}</span>
                <SearchIcon size={18} />
              </button>

              <button
                onClick={onOpenContext}
                disabled={!selectedGroupId}
                className={classNames(
                  headerIconButtonClass,
                  "text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)]"
                )}
                title={t('context')}
              >
                <span className="sr-only">{t('context')}</span>
                <ClipboardIcon size={18} />
              </button>

              <div className="w-px h-4 mx-1 bg-black/10 dark:bg-white/10" />

              <button
                onClick={onStartGroup}
                disabled={!selectedGroupId || busy === "group-start" || actors.length === 0}
                className={classNames(
                  headerIconButtonClass,
                  "border shadow-sm hover:-translate-y-px active:translate-y-0",
                  "border-emerald-200/70 bg-emerald-50/75 text-emerald-700 shadow-emerald-100/80 hover:bg-emerald-100/80 hover:shadow-emerald-200/70",
                  "dark:border-[var(--glass-border-subtle)] dark:bg-[var(--glass-panel-bg)] dark:text-emerald-400 dark:shadow-none dark:hover:text-emerald-300"
                )}
                title={t('launchAllAgents')}
              >
                <span className="sr-only">{t('launchAllAgents')}</span>
                <RocketIcon size={18} className="drop-shadow-[0_1px_3px_rgba(16,185,129,0.22)]" />
              </button>

              {groupDoc?.state === "paused" ? (
                <button
                  onClick={() => void onSetGroupState("active")}
                  disabled={!selectedGroupId || busy === "group-state"}
                  className={classNames(
                    headerIconButtonClass,
                    "text-amber-600 dark:text-amber-400"
                  )}
                  title={t('resumeDelivery')}
                >
                  <span className="sr-only">{t('resumeDelivery')}</span>
                  <PlayIcon size={18} />
                </button>
              ) : (
                <button
                  onClick={() => void onSetGroupState("paused")}
                  disabled={!selectedGroupId || busy === "group-state"}
                  className={classNames(
                    headerIconButtonClass,
                    "text-gray-400 hover:text-amber-600 dark:text-slate-400 dark:hover:text-amber-300"
                  )}
                  title={t('pauseDelivery')}
                >
                  <span className="sr-only">{t('pauseDelivery')}</span>
                  <PauseIcon size={18} />
                </button>
              )}

              <button
                onClick={onStopGroup}
                disabled={!selectedGroupId || busy === "group-stop"}
                className={classNames(
                  headerIconButtonClass,
                  "text-gray-400 hover:text-rose-600 dark:text-slate-400 dark:hover:text-rose-400"
                )}
                title={t('stopAllAgents')}
              >
                <span className="sr-only">{t('stopAllAgents')}</span>
                <StopIcon size={18} />
              </button>
            </div>

            <div className="hidden md:flex items-center gap-1">
              <ThemeToggleCompact theme={theme} onThemeChange={onThemeChange} isDark={isDark} />
            </div>

            <button
              onClick={onOpenSettings}
              disabled={!selectedGroupId}
              className={classNames(
                "hidden md:flex",
                headerIconButtonClass,
                "text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]"
              )}
              title={t('settings')}
            >
              <SettingsIcon size={18} />
            </button>

            <div className="hidden md:block w-px h-4 bg-black/10 dark:bg-white/10" />
            <div className="hidden md:block">
              <LanguageSwitcher isDark={isDark} />
            </div>

            <button
              className={classNames(
                "md:hidden",
                headerIconButtonClass,
                "text-[var(--color-text-muted)]"
              )}
              onClick={onOpenMobileMenu}
              title={t('menu')}
            >
              <MoreIcon size={18} />
            </button>
          </>
        )}
      </div>

    </header>
  );
}
