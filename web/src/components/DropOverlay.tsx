import { useTranslation } from 'react-i18next';
import { classNames } from "../utils/classNames";

export interface DropOverlayProps {
  isOpen: boolean;
  isDark: boolean;
  maxFileMb: number;
}

export function DropOverlay({ isOpen, isDark, maxFileMb }: DropOverlayProps) {
  const { t } = useTranslation('layout');

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-overlay">
      <div className={classNames("absolute inset-0 backdrop-blur-sm", isDark ? "bg-black/60" : "bg-black/40")} aria-hidden="true" />
      <div className="absolute inset-0 flex items-center justify-center p-6">
        <div
          className={classNames(
            "w-full max-w-sm rounded-2xl border px-6 py-5 text-center shadow-2xl",
            isDark ? "bg-slate-900/90 border-slate-700 text-slate-100" : "bg-white/90 border-gray-200 text-gray-900"
          )}
          role="dialog"
          aria-label={t('dropFilesToAttach')}
        >
          <div className="text-3xl mb-2">📎</div>
          <div className="text-sm font-semibold">{t('dropFilesToAttach')}</div>
          <div className={classNames("text-xs mt-1", isDark ? "text-slate-400" : "text-gray-500")}>
            {t('dropFilesHint')}
          </div>
          <div className={classNames("text-[11px] mt-3", isDark ? "text-slate-500" : "text-gray-500")}>{t('maxFileSize', { size: maxFileMb })}</div>
        </div>
      </div>
    </div>
  );
}
