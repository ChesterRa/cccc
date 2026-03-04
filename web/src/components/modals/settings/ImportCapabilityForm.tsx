import { useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import type { CapabilityImportRecord } from "../../../types";
import { cardClass, inputClass } from "./types";
import { validateCapabilityImportResult } from "./capabilityMutation";

type InstallMode = "command" | "package" | "remote_only";

interface ImportCapabilityFormProps {
  isDark: boolean;
  groupId?: string;
  onImported?: () => void;
}

const INSTALL_MODES: InstallMode[] = ["command", "package", "remote_only"];

export function ImportCapabilityForm({ isDark, groupId, onImported }: ImportCapabilityFormProps) {
  const { t } = useTranslation("settings");
  const [installMode, setInstallMode] = useState<InstallMode>("command");
  const [command, setCommand] = useState("");
  const [packageName, setPackageName] = useState("");
  const [remoteUrl, setRemoteUrl] = useState("");
  const [capabilityId, setCapabilityId] = useState("");
  const [enableAfter, setEnableAfter] = useState(true);
  const [busy, setBusy] = useState(false);
  const [dryRunResult, setDryRunResult] = useState<Record<string, unknown> | null>(null);
  const [err, setErr] = useState("");
  const [success, setSuccess] = useState("");

  const inferKind = (id: string): "mcp_toolpack" | "skill" => {
    if (id.startsWith("skill:")) return "skill";
    return "mcp_toolpack";
  };

  const buildRecord = (): CapabilityImportRecord => {
    const capId = capabilityId.trim();
    const rec: CapabilityImportRecord = {
      capability_id: capId,
      kind: inferKind(capId),
      install_mode: installMode,
    };
    if (installMode === "command" && command.trim()) rec.install_spec = { command: command.trim() };
    if (installMode === "package" && packageName.trim()) rec.install_spec = { package: packageName.trim() };
    if (installMode === "remote_only" && remoteUrl.trim()) rec.install_spec = { url: remoteUrl.trim() };
    return rec;
  };

  const hasInput = (): boolean => {
    if (!capabilityId.trim()) return false;
    if (installMode === "command") return !!command.trim();
    if (installMode === "package") return !!packageName.trim();
    if (installMode === "remote_only") return !!remoteUrl.trim();
    return false;
  };

  const handleDryRun = async () => {
    if (!groupId || !hasInput()) return;
    setBusy(true);
    setErr("");
    setSuccess("");
    setDryRunResult(null);
    try {
      const resp = await api.importCapability(groupId, buildRecord(), { dryRun: true });
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilities.importForm.failedDryRun"));
        return;
      }
      setDryRunResult(resp.result as Record<string, unknown>);
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("capabilities.importForm.failedDryRun"));
    } finally {
      setBusy(false);
    }
  };

  const handleImport = async () => {
    if (!groupId || !hasInput()) return;
    setBusy(true);
    setErr("");
    setSuccess("");
    setDryRunResult(null);
    try {
      const resp = await api.importCapability(groupId, buildRecord(), {
        dryRun: false,
        enableAfterImport: enableAfter,
        scope: "group",
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilities.importForm.failedImport"));
        return;
      }
      const validation = validateCapabilityImportResult(resp.result, { enableAfterImport: enableAfter });
      if (!validation.ok) {
        setErr(
          validation.reason
            ? t("capabilities.operationFailedReason", { reason: validation.reason })
            : t("capabilities.importForm.failedImport")
        );
        return;
      }
      setSuccess(t("capabilities.importForm.success"));
      setCommand("");
      setPackageName("");
      setRemoteUrl("");
      setCapabilityId("");
      onImported?.();
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("capabilities.importForm.failedImport"));
    } finally {
      setBusy(false);
    }
  };

  if (!groupId) return null;

  return (
    <div className={cardClass(isDark)}>
      <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>
        {t("capabilities.importForm.title")}
      </div>
      <div className={`text-xs mt-1 mb-3 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
        {t("capabilities.importForm.subtitle")}
      </div>

      {/* Install Mode Tabs */}
      <div className="inline-flex rounded-lg border overflow-hidden mb-3">
        {INSTALL_MODES.map((mode) => (
          <button
            key={mode}
            type="button"
            onClick={() => { setInstallMode(mode); setDryRunResult(null); setErr(""); setSuccess(""); }}
            className={`px-3 py-2 text-xs min-h-[36px] ${
              installMode === mode
                ? isDark ? "bg-slate-700 text-slate-100" : "bg-gray-100 text-gray-900"
                : isDark ? "bg-slate-900 text-slate-300" : "bg-white text-gray-700"
            } ${mode !== "command" ? `border-l ${isDark ? "border-slate-600" : "border-gray-200"}` : ""}`}
          >
            {t(`capabilities.importForm.mode_${mode}`)}
          </button>
        ))}
      </div>

      {/* Mode-specific input */}
      {installMode === "command" ? (
        <input
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          placeholder={t("capabilities.importForm.commandPlaceholder")}
          className={inputClass(isDark)}
        />
      ) : null}
      {installMode === "package" ? (
        <input
          value={packageName}
          onChange={(e) => setPackageName(e.target.value)}
          placeholder={t("capabilities.importForm.packagePlaceholder")}
          className={inputClass(isDark)}
        />
      ) : null}
      {installMode === "remote_only" ? (
        <input
          value={remoteUrl}
          onChange={(e) => setRemoteUrl(e.target.value)}
          placeholder={t("capabilities.importForm.remotePlaceholder")}
          className={inputClass(isDark)}
        />
      ) : null}

      {/* Required capability_id */}
      <input
        value={capabilityId}
        onChange={(e) => setCapabilityId(e.target.value)}
        placeholder={t("capabilities.importForm.capIdPlaceholder")}
        className={`mt-2 ${inputClass(isDark)}`}
      />

      {/* Enable after import */}
      <div className="mt-3 flex flex-wrap items-center gap-3">
        <label className={`flex items-center gap-2 text-xs ${isDark ? "text-slate-300" : "text-gray-700"}`}>
          <input
            type="checkbox"
            checked={enableAfter}
            onChange={(e) => setEnableAfter(e.target.checked)}
            className="rounded"
          />
          {t("capabilities.importForm.enableAfter")}
        </label>
      </div>

      {/* Action buttons */}
      <div className="mt-3 flex gap-2">
        <button
          type="button"
          className={`px-3 py-2 rounded-lg text-xs min-h-[36px] ${
            isDark ? "bg-slate-800 text-slate-200 hover:bg-slate-700" : "bg-white border border-gray-200 text-gray-700 hover:bg-gray-50"
          } disabled:opacity-50`}
          disabled={busy || !hasInput()}
          onClick={() => void handleDryRun()}
        >
          {t("capabilities.importForm.dryRun")}
        </button>
        <button
          type="button"
          className={`px-3 py-2 rounded-lg text-xs min-h-[36px] bg-emerald-600 hover:bg-emerald-500 text-white disabled:opacity-50`}
          disabled={busy || !hasInput()}
          onClick={() => void handleImport()}
        >
          {busy ? t("common:loading") : t("capabilities.importForm.importAndEnable")}
        </button>
      </div>

      {/* Dry Run Preview */}
      {dryRunResult ? (
        <div className={`mt-3 rounded-lg border p-2 ${isDark ? "border-slate-700 bg-slate-900" : "border-gray-200 bg-white"}`}>
          <div className={`text-xs font-medium mb-1 ${isDark ? "text-slate-300" : "text-gray-700"}`}>
            {t("capabilities.importForm.dryRunPreview")}
          </div>
          <pre className={`text-[11px] whitespace-pre-wrap overflow-auto max-h-[200px] ${isDark ? "text-slate-400" : "text-gray-600"}`}>
            {JSON.stringify(dryRunResult, null, 2)}
          </pre>
        </div>
      ) : null}

      {/* Feedback */}
      {err ? (
        <div className={`mt-2 text-xs ${isDark ? "text-rose-300" : "text-rose-700"}`} role="alert">{err}</div>
      ) : null}
      {success ? (
        <div className={`mt-2 text-xs ${isDark ? "text-emerald-300" : "text-emerald-700"}`}>{success}</div>
      ) : null}
    </div>
  );
}
