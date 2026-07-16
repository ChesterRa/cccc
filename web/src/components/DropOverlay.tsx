import { useTranslation } from "react-i18next";

export interface DropOverlayProps {
  isOpen: boolean;
  isDark: boolean;
  maxFileMb: number;
}

export function DropOverlay({ isOpen, isDark: _isDark, maxFileMb }: DropOverlayProps) {
  const { t } = useTranslation("layout");

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-overlay">
      <div className="glass-overlay absolute inset-0" aria-hidden="true" />
      <div className="absolute inset-0 flex items-center justify-center p-6">
        <div
          className="glass-modal w-full max-w-sm px-6 py-5 text-center text-[var(--color-text-primary)]"
          role="dialog"
          aria-label={t("dropFilesToAttach")}
        >
          <div className="text-3xl mb-2">📎</div>
          <div className="text-sm font-semibold">{t("dropFilesToAttach")}</div>
          <div className="text-xs mt-1 text-[var(--color-text-tertiary)]">{t("dropFilesHint")}</div>
          <div className="text-[11px] mt-3 text-[var(--color-text-muted)]">
            {t("maxFileSize", { size: maxFileMb })}
          </div>
        </div>
      </div>
    </div>
  );
}
