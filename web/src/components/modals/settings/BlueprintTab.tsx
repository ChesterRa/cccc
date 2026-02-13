import { useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import { cardClass, labelClass, primaryButtonClass } from "./types";
import { TemplatePreviewDetails } from "../../TemplatePreviewDetails";
import type { TemplatePreviewDetailsProps } from "../../TemplatePreviewDetails";

interface BlueprintTabProps {
  isDark: boolean;
  groupId?: string;
  groupTitle?: string;
}

type TemplatePreviewResult = {
  template?: TemplatePreviewDetailsProps["template"];
  diff?: NonNullable<TemplatePreviewDetailsProps["diff"]>;
};

function downloadTextFile(filename: string, text: string) {
  const blob = new Blob([text], { type: "text/yaml;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

export function BlueprintTab({ isDark, groupId, groupTitle }: BlueprintTabProps) {
  const { t } = useTranslation("settings");
  const [file, setFile] = useState<File | null>(null);
  const [preview, setPreview] = useState<TemplatePreviewResult | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [exportInfo, setExportInfo] = useState("");

  const canUse = !!groupId;

  const loadPreview = async (f: File) => {
    if (!groupId) return;
    setBusy(true);
    setErr("");
    setPreview(null);
    try {
      const resp = await api.previewGroupTemplate(groupId, f);
      if (!resp.ok) {
        setErr(resp.error?.message || t("blueprint.failedToPreview"));
        return;
      }
      setPreview(resp.result as TemplatePreviewResult);
    } catch {
      setErr(t("blueprint.failedToPreview"));
    } finally {
      setBusy(false);
    }
  };

  const handleExport = async () => {
    if (!groupId) return;
    setBusy(true);
    setErr("");
    setExportInfo("");
    try {
      const resp = await api.exportGroupTemplate(groupId);
      if (!resp.ok) {
        setErr(resp.error?.message || t("blueprint.failedToExport"));
        return;
      }
      const filename = resp.result?.filename || `cccc-group-template--${groupTitle || groupId}.yaml`;
      downloadTextFile(filename, String(resp.result?.template || ""));
      setExportInfo(t("blueprint.downloaded"));
      window.setTimeout(() => setExportInfo(""), 1200);
    } catch {
      setErr(t("blueprint.failedToExport"));
    } finally {
      setBusy(false);
    }
  };

  const handleImportReplace = async () => {
    if (!groupId || !file) return;
    const ok = window.confirm(t("blueprint.replaceConfirm", { filename: file.name }));
    if (!ok) return;

    setBusy(true);
    setErr("");
    try {
      const resp = await api.importGroupTemplateReplace(groupId, file);
      if (!resp.ok) {
        setErr(resp.error?.message || t("blueprint.failedToImport"));
        return;
      }
      setFile(null);
      setPreview(null);
      setExportInfo(t("blueprint.applied"));
      window.setTimeout(() => setExportInfo(""), 1200);
    } catch {
      setErr(t("blueprint.failedToImport"));
    } finally {
      setBusy(false);
    }
  };

  if (!canUse) {
    return (
      <div className={`text-sm ${isDark ? "text-slate-300" : "text-gray-700"}`}>
        {t("blueprint.openFromGroup")}
      </div>
    );
  }

  const diff = preview?.diff;
  const tpl = preview?.template;

  return (
    <div className="space-y-4">
      {err && <div className={`text-sm ${isDark ? "text-rose-300" : "text-red-600"}`}>{err}</div>}

      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("blueprint.exportTitle")}</div>
        <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
          {t("blueprint.exportDescription")}
        </div>
        <div className="mt-3 flex items-center gap-2">
          <button className={primaryButtonClass(busy)} onClick={handleExport} disabled={busy}>
            {t("blueprint.exportBlueprint")}
          </button>
          {exportInfo && <div className={`text-xs ${isDark ? "text-emerald-300" : "text-emerald-700"}`}>{exportInfo}</div>}
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("blueprint.importTitle")}</div>
        <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
          {t("blueprint.importDescription")}
        </div>

        <div className="mt-3">
          <label className={labelClass(isDark)}>{t("blueprint.blueprintFile")}</label>
          <input
            key={file ? file.name : "none"}
            type="file"
            accept=".yaml,.yml,.json"
            className={`text-sm ${isDark ? "text-slate-300" : "text-gray-700"}`}
            disabled={busy}
            onChange={(e) => {
              const f = e.target.files && e.target.files.length > 0 ? e.target.files[0] : null;
              setFile(f);
              setPreview(null);
              setErr("");
              if (f) void loadPreview(f);
            }}
          />
        </div>

        {busy && <div className={`mt-2 text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("blueprint.working")}</div>}

        {tpl && diff && (
          <div className="mt-3">
            <TemplatePreviewDetails isDark={isDark} template={tpl} diff={diff} wrap={false} />
          </div>
        )}

        <div className="mt-3 flex items-center gap-2">
          <button
            className={primaryButtonClass(busy)}
            onClick={handleImportReplace}
            disabled={busy || !file || !preview}
            title={!preview ? t("blueprint.pickFileFirst") : ""}
          >
            {t("blueprint.applyReplace")}
          </button>
          <button
            type="button"
            className={`px-4 py-2 rounded-lg text-sm min-h-[44px] transition-colors disabled:opacity-50 ${
              isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
            }`}
            disabled={busy}
            onClick={() => {
              setFile(null);
              setPreview(null);
              setErr("");
            }}
          >
            {t("common:close")}
          </button>
        </div>
      </div>
    </div>
  );
}
