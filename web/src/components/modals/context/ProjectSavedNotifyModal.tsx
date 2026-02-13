import { useTranslation } from "react-i18next";
import { MarkdownRenderer } from "../../MarkdownRenderer";
import { classNames } from "../../../utils/classNames";

interface ProjectSavedNotifyModalProps {
  isOpen: boolean;
  isDark: boolean;
  projectPathLabel: string;
  notifyMessage: string;
  notifyAgents: boolean;
  notifyBusy: boolean;
  notifyError: string;
  onChangeNotifyAgents: (checked: boolean) => void;
  onDone: () => void;
  onClose: () => void;
}

export function ProjectSavedNotifyModal({
  isOpen,
  isDark,
  projectPathLabel,
  notifyMessage,
  notifyAgents,
  notifyBusy,
  notifyError,
  onChangeNotifyAgents,
  onDone,
  onClose,
}: ProjectSavedNotifyModalProps) {
  const { t } = useTranslation("modals");
  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-overlay flex items-center justify-center p-4 animate-fade-in">
      <div
        className={isDark ? "absolute inset-0 bg-black/70" : "absolute inset-0 bg-black/50"}
        onPointerDown={(e) => {
          if (e.target !== e.currentTarget) return;
          if (!notifyBusy) onClose();
        }}
        aria-hidden="true"
      />
      <div
        className={`relative w-full max-w-md rounded-xl border shadow-2xl p-4 ${
          isDark ? "bg-slate-900 border-slate-700" : "bg-white border-gray-200"
        }`}
        role="dialog"
        aria-modal="true"
        aria-label={t("projectSaved.projectUpdatedAria")}
      >
        <div className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>{t("projectSaved.title")}</div>
        <div className={`text-xs mt-1 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{projectPathLabel}</div>

        {notifyError ? (
          <div
            className={`mt-3 text-xs rounded-lg border px-3 py-2 ${
              isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-300 bg-rose-50 text-rose-700"
            }`}
          >
            {notifyError}
          </div>
        ) : null}

        <label className={`mt-3 flex items-center gap-2 text-sm ${isDark ? "text-slate-200" : "text-gray-800"}`}>
          <input
            type="checkbox"
            checked={notifyAgents}
            onChange={(e) => onChangeNotifyAgents(e.target.checked)}
            disabled={notifyBusy}
          />
          {t("projectSaved.notifyAgents")}
        </label>

        <MarkdownRenderer
          content={notifyMessage}
          isDark={isDark}
          className={classNames("mt-2 text-[11px] rounded-lg px-3 py-2", isDark ? "bg-slate-800/60 text-slate-300" : "bg-gray-50 text-gray-700")}
        />

        <div className="mt-3 flex gap-2">
          <button
            onClick={onDone}
            disabled={notifyBusy}
            className="flex-1 px-4 py-2 bg-blue-600 hover:bg-blue-500 text-white text-sm rounded-lg disabled:opacity-50 min-h-[44px] transition-colors"
          >
            {notifyBusy ? t("projectSaved.working") : t("common:done")}
          </button>
          <button
            onClick={onClose}
            disabled={notifyBusy}
            className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors disabled:opacity-50 ${
              isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
            }`}
          >
            {t("common:close")}
          </button>
        </div>
      </div>
    </div>
  );
}
