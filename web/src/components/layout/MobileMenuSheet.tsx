import { useTranslation } from 'react-i18next';
import { Actor, GroupDoc } from "../../types";
import { getGroupStatus, getGroupStatusLight } from "../../utils/groupStatus";
import { classNames } from "../../utils/classNames";
import { useModalA11y } from "../../hooks/useModalA11y";
import {
  SearchIcon,
  ClipboardIcon,
  SettingsIcon,
  SunIcon,
  MoonIcon,
  EditIcon,
  PlayIcon,
  StopIcon,
  PauseIcon,
  CloseIcon,
} from "../Icons";

export interface MobileMenuSheetProps {
  isOpen: boolean;
  isDark: boolean;
  selectedGroupId: string;
  groupDoc: GroupDoc | null;
  selectedGroupRunning: boolean;
  actors: Actor[];
  busy: string;
  onClose: () => void;
  onToggleTheme: () => void;
  onOpenSearch: () => void;
  onOpenContext: () => void;
  onOpenSettings: () => void;
  onOpenGroupEdit: () => void;
  onStartGroup: () => void;
  onStopGroup: () => void;
  onSetGroupState: (state: "active" | "paused" | "idle") => void | Promise<void>;
}

export function MobileMenuSheet({
  isOpen,
  isDark,
  selectedGroupId,
  groupDoc,
  selectedGroupRunning,
  actors,
  busy,
  onClose,
  onToggleTheme,
  onOpenSearch,
  onOpenContext,
  onOpenSettings,
  onOpenGroupEdit,
  onStartGroup,
  onStopGroup,
  onSetGroupState,
}: MobileMenuSheetProps) {
  const { modalRef } = useModalA11y(isOpen, onClose);
  const { t } = useTranslation('layout');
  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 md:hidden animate-fade-in">
      <div
        className="absolute inset-0 glass-overlay"
        onPointerDown={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
        aria-hidden="true"
      />

      <div
        ref={modalRef}
        className="absolute bottom-0 left-0 right-0 rounded-t-3xl glass-modal animate-slide-up transform transition-transform"
        role="dialog"
        aria-modal="true"
        aria-label={t('menu')}
      >
        <div className="flex justify-center pt-3 pb-1" onClick={onClose}>
          <div className={`w-12 h-1.5 rounded-full ${isDark ? "bg-white/20" : "bg-black/15"}`} />
        </div>

        <div className="px-6 pb-4 flex items-center justify-between gap-3">
          <div className="min-w-0">
            <div className={classNames("text-lg font-bold truncate", isDark ? "text-slate-100" : "text-gray-900")}>
              {groupDoc?.title || (selectedGroupId ? selectedGroupId : t('menu'))}
            </div>
            {selectedGroupId && groupDoc && (
              <div className="flex items-center gap-2 mt-1">
                <span
                  className={classNames(
                    "text-xs px-2 py-0.5 rounded-full font-medium",
                    isDark ? getGroupStatus(selectedGroupRunning, groupDoc.state).pillClass : getGroupStatusLight(selectedGroupRunning, groupDoc.state).pillClass
                  )}
                >
                  {(isDark ? getGroupStatus(selectedGroupRunning, groupDoc.state) : getGroupStatusLight(selectedGroupRunning, groupDoc.state)).label}
                </span>
              </div>
            )}
          </div>
          <button
            onClick={onClose}
            className={classNames(
              "p-2 rounded-full transition-colors glass-btn",
              isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-400 hover:text-gray-600"
            )}
            aria-label={t('closeMenu')}
          >
            <CloseIcon size={20} />
          </button>
        </div>

        <div className="p-4 space-y-2 safe-area-inset-bottom">
          {!selectedGroupId && (
            <div className={classNames("text-sm px-1 pb-2", isDark ? "text-slate-400" : "text-gray-500")}>
              {t('selectGroupToEnable')}
            </div>
          )}

          <button
            className={classNames(
              "w-full flex items-center justify-center gap-3 px-4 py-3.5 rounded-2xl text-sm font-medium transition-all min-h-[52px] disabled:opacity-50 glass-btn",
              isDark ? "text-slate-200" : "text-gray-800"
            )}
            onClick={() => {
              onClose();
              onOpenSearch();
            }}
            disabled={!selectedGroupId}
          >
            <SearchIcon size={18} />
            <span>{t('searchMessagesButton')}</span>
          </button>

          <div className="grid grid-cols-2 gap-2">
            <button
              className={classNames(
                "w-full flex items-center justify-center gap-2 px-4 py-3.5 rounded-2xl text-sm font-medium transition-all min-h-[52px] disabled:opacity-50 glass-btn",
                isDark ? "text-slate-200" : "text-gray-800"
              )}
              onClick={() => {
                onClose();
                onOpenContext();
              }}
              disabled={!selectedGroupId}
            >
              <ClipboardIcon size={18} />
              <span>{t('contextButton')}</span>
            </button>

            <button
              className={classNames(
                "w-full flex items-center justify-center gap-2 px-4 py-3.5 rounded-2xl text-sm font-medium transition-all min-h-[52px] disabled:opacity-50 glass-btn",
                isDark ? "text-slate-200" : "text-gray-800"
              )}
              onClick={() => {
                onClose();
                onOpenSettings();
              }}
              disabled={!selectedGroupId}
            >
              <SettingsIcon size={18} />
              <span>{t('settingsButton')}</span>
            </button>
          </div>

          <button
            className={classNames(
              "w-full flex items-center justify-center gap-3 px-4 py-3.5 rounded-2xl text-sm font-medium transition-all min-h-[52px] disabled:opacity-50 glass-btn",
              isDark ? "text-slate-200" : "text-gray-800"
            )}
            onClick={onToggleTheme}
          >
            {isDark ? <SunIcon size={18} /> : <MoonIcon size={18} />}
            <span>{isDark ? t('lightMode') : t('darkMode')}</span>
          </button>

          <button
            className={classNames(
              "w-full flex items-center justify-center gap-3 px-4 py-3.5 rounded-2xl text-sm font-medium transition-all min-h-[52px] disabled:opacity-50 glass-btn",
              isDark ? "text-slate-200" : "text-gray-800"
            )}
            onClick={() => {
              onClose();
              onOpenGroupEdit();
            }}
            disabled={!selectedGroupId}
          >
            <EditIcon size={18} />
            <span>{t('editGroupDetails')}</span>
          </button>

          <div className={classNames("h-px my-3 mx-2", isDark ? "bg-white/10" : "bg-black/10")} />

          <div className="grid grid-cols-2 gap-2">
            <button
              className={classNames(
                "w-full flex flex-col items-center justify-center gap-2 px-2 py-3 rounded-2xl text-sm font-medium transition-all min-h-[64px] disabled:opacity-50",
                isDark
                  ? "glass-btn-accent text-emerald-300"
                  : "glass-btn-accent text-emerald-700"
              )}
              style={{
                '--glass-accent-bg': isDark ? 'rgba(16, 185, 129, 0.15)' : 'rgba(16, 185, 129, 0.1)',
                '--glass-accent-border': isDark ? 'rgba(16, 185, 129, 0.25)' : 'rgba(16, 185, 129, 0.2)',
                '--glass-accent-glow': isDark ? '0 0 20px rgba(16, 185, 129, 0.15)' : '0 0 16px rgba(16, 185, 129, 0.1)',
              } as React.CSSProperties}
              onClick={() => {
                onClose();
                onStartGroup();
              }}
              disabled={!selectedGroupId || busy === "group-start" || actors.length === 0}
            >
              <PlayIcon size={20} />
              <span>{t('launchAll')}</span>
            </button>

            <button
              className={classNames(
                "w-full flex flex-col items-center justify-center gap-2 px-2 py-3 rounded-2xl text-sm font-medium transition-all min-h-[64px] disabled:opacity-50 glass-btn",
                isDark ? "text-slate-300" : "text-gray-600"
              )}
              onClick={() => {
                onClose();
                onStopGroup();
              }}
              disabled={!selectedGroupId || busy === "group-stop"}
            >
              <StopIcon size={20} />
              <span>{t('quitAll')}</span>
            </button>
          </div>

          {groupDoc?.state === "paused" ? (
            <button
              className={classNames(
                "w-full flex items-center justify-center gap-2 px-4 py-3.5 rounded-2xl text-sm font-medium transition-all min-h-[52px] disabled:opacity-50",
                isDark
                  ? "glass-btn-accent text-amber-300"
                  : "glass-btn-accent text-amber-700"
              )}
              style={{
                '--glass-accent-bg': isDark ? 'rgba(245, 158, 11, 0.15)' : 'rgba(245, 158, 11, 0.1)',
                '--glass-accent-border': isDark ? 'rgba(245, 158, 11, 0.25)' : 'rgba(245, 158, 11, 0.2)',
                '--glass-accent-glow': isDark ? '0 0 20px rgba(245, 158, 11, 0.15)' : '0 0 16px rgba(245, 158, 11, 0.1)',
              } as React.CSSProperties}
              onClick={() => {
                onClose();
                void onSetGroupState("active");
              }}
              disabled={!selectedGroupId || busy === "group-state"}
            >
              <PlayIcon size={18} />
              <span>{t('resumeMessageDelivery')}</span>
            </button>
          ) : (
            <button
              className={classNames(
                "w-full flex items-center justify-center gap-2 px-4 py-3.5 rounded-2xl text-sm font-medium transition-all min-h-[52px] disabled:opacity-50 glass-btn",
                isDark ? "text-slate-300" : "text-gray-600"
              )}
              onClick={() => {
                onClose();
                void onSetGroupState("paused");
              }}
              disabled={!selectedGroupId || busy === "group-state"}
            >
              <PauseIcon size={18} />
              <span>{t('pauseMessageDelivery')}</span>
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
