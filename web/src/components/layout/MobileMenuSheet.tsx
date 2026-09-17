import "./MobileMenuSheet.css";
import { useTranslation } from "react-i18next";
import { GroupDoc, TextScale, Theme } from "../../types";
import { getGroupStatusFromSource } from "../../utils/groupStatus";
import { classNames } from "../../utils/classNames";
import { useModalA11y } from "../../hooks/useModalA11y";
import { AppearancePreferences } from "./AppearancePreferences";
import {
  SearchIcon,
  ClipboardIcon,
  FolderIcon,
  SettingsIcon,
  AccountIcon,
  EditIcon,
  CloseIcon,
} from "../Icons";
import { GroupStatusIndicator } from "./GroupStatusIndicator";

export interface MobileMenuSheetProps {
  isOpen: boolean;
  theme: Theme;
  textScale: TextScale;
  selectedGroupId: string;
  groupDoc: GroupDoc | null;
  selectedGroupRunning: boolean;
  onClose: () => void;
  onThemeChange: (theme: Theme) => void;
  onTextScaleChange: (scale: TextScale) => void;
  onOpenSearch: () => void;
  onOpenContext: () => void;
  /** Opens the workspace file tree as a full-screen surface; absent when unavailable. */
  onOpenFiles?: () => void;
  onOpenSettings: () => void;
  canAccessAccount: boolean;
  accountLabel?: string | null;
  onOpenAccount: () => void;
  onOpenGroupEdit?: () => void;
}

export function MobileMenuSheet({
  isOpen,
  theme,
  textScale,
  selectedGroupId,
  groupDoc,
  selectedGroupRunning,
  onClose,
  onThemeChange,
  onTextScaleChange,
  onOpenSearch,
  onOpenContext,
  onOpenFiles,
  onOpenSettings,
  canAccessAccount,
  accountLabel,
  onOpenAccount,
  onOpenGroupEdit,
}: MobileMenuSheetProps) {
  const { modalRef } = useModalA11y(isOpen, onClose);
  const { t } = useTranslation("layout");
  const selectedStatus = selectedGroupId
    ? getGroupStatusFromSource({
        running: selectedGroupRunning,
        state: groupDoc?.state,
        runtime_status: groupDoc?.runtime_status,
      })
    : null;
  const sectionCardClass =
    "rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-2 shadow-sm backdrop-blur-xl";
  const sectionTitleClass =
    "px-2.5 pb-1 text-xs font-semibold uppercase tracking-[0.12em] text-[var(--color-text-muted)]";
  const rowButtonClass =
    "w-full flex items-center justify-between gap-3 rounded-xl px-3.5 py-3 text-sm transition-all text-[var(--color-text-primary)] hover:bg-black/5 disabled:opacity-45 dark:hover:bg-white/6";

  if (!isOpen) return null;

  return (
    <div className="mobile-menu-viewport fixed inset-0 z-50 animate-fade-in">
      <div
        className="absolute inset-0 glass-overlay"
        onPointerDown={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
        aria-hidden="true"
      />

      <div
        ref={modalRef}
        className="mobile-menu-panel rounded-t-3xl glass-modal animate-slide-up transform transition-transform"
        role="dialog"
        aria-modal="true"
        aria-label={t("menu")}
      >
        <div className="flex justify-center pt-3 pb-1 md:hidden" onClick={onClose}>
          <div className="w-12 h-1.5 rounded-full bg-black/15 dark:bg-white/20" />
        </div>

        <div className="px-6 pb-4 md:pt-5 flex items-center justify-between gap-3">
          <div className="min-w-0">
            <div
              className={classNames(
                "text-lg font-bold truncate",
                "text-[var(--color-text-primary)]",
              )}
            >
              {groupDoc?.title || (selectedGroupId ? selectedGroupId : t("menu"))}
            </div>
            {selectedStatus && (
              <div className="flex items-center gap-2 mt-1">
                <GroupStatusIndicator status={selectedStatus} variant="badge" />
              </div>
            )}
          </div>
          <button
            onClick={onClose}
            className={classNames(
              "p-2 rounded-full transition-colors glass-btn",
              "text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)]",
            )}
            aria-label={t("closeMenu")}
          >
            <CloseIcon size={20} />
          </button>
        </div>

        <div className="mobile-menu-content p-4 space-y-4">
          {!selectedGroupId && (
            <div className={classNames("text-sm px-1 pb-2", "text-[var(--color-text-tertiary)]")}>
              {t("selectGroupToEnable")}
            </div>
          )}

          <button
            className={classNames(
              "w-full flex items-center justify-between gap-3 px-4 py-3.5 rounded-2xl text-sm font-medium transition-all min-h-[52px] disabled:opacity-50 glass-btn shadow-sm",
              "text-[var(--color-text-primary)]",
            )}
            onClick={() => {
              onClose();
              onOpenSearch();
            }}
            disabled={!selectedGroupId}
          >
            <div className="flex items-center gap-3">
              <SearchIcon size={18} />
              <span>{t("searchMessagesButton")}</span>
            </div>
          </button>

          <section className={sectionCardClass}>
            <div className={sectionTitleClass}>{t("workspaceSection")}</div>
            {canAccessAccount ? (
              <button
                className={rowButtonClass}
                onClick={() => {
                  onClose();
                  onOpenAccount();
                }}
              >
                <div className="flex items-center gap-3">
                  <AccountIcon size={18} />
                  <span className="min-w-0 text-left">
                    <span className="block">{t("account")}</span>
                    {accountLabel ? (
                      <span
                        className="block max-w-56 truncate text-xs text-[var(--color-text-muted)]"
                        title={t("linkedAccount", { account: accountLabel })}
                      >
                        {accountLabel}
                      </span>
                    ) : null}
                  </span>
                </div>
              </button>
            ) : null}
            <button
              className={rowButtonClass}
              onClick={() => {
                onClose();
                onOpenContext();
              }}
              disabled={!selectedGroupId}
            >
              <div className="flex items-center gap-3">
                <ClipboardIcon size={18} />
                <span>{t("contextButton")}</span>
              </div>
            </button>
            {onOpenFiles ? (
              <button
                className={rowButtonClass}
                onClick={() => {
                  onClose();
                  onOpenFiles();
                }}
                disabled={!selectedGroupId}
              >
                <div className="flex items-center gap-3">
                  <FolderIcon size={18} />
                  <span>{t("workspaceFilesButton")}</span>
                </div>
              </button>
            ) : null}
            <button
              className={rowButtonClass}
              onClick={() => {
                onClose();
                onOpenSettings();
              }}
              disabled={!selectedGroupId && !canAccessAccount}
            >
              <div className="flex items-center gap-3">
                <SettingsIcon size={18} />
                <span>{t("settingsButton")}</span>
              </div>
            </button>
            {onOpenGroupEdit ? (
              <button
                className={rowButtonClass}
                onClick={() => {
                  onClose();
                  onOpenGroupEdit();
                }}
                disabled={!selectedGroupId}
              >
                <div className="flex items-center gap-3">
                  <EditIcon size={18} />
                  <span>{t("editGroupDetails")}</span>
                </div>
              </button>
            ) : null}
          </section>

          <section className={sectionCardClass}>
            <AppearancePreferences
              theme={theme}
              textScale={textScale}
              onThemeChange={onThemeChange}
              onTextScaleChange={onTextScaleChange}
            />
          </section>
        </div>
      </div>
    </div>
  );
}
