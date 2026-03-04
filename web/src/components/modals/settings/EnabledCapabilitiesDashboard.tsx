import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import type { CapabilityEnabledEntry } from "../../../types";
import { cardClass } from "./types";
import { validateCapabilityToggleResult } from "./capabilityMutation";

interface EnabledCapabilitiesDashboardProps {
  isDark: boolean;
  groupId?: string;
  refreshKey?: number;
}

function formatTtl(expiresAt?: string): string {
  if (!expiresAt) return "";
  const diff = new Date(expiresAt).getTime() - Date.now();
  if (diff <= 0) return "expired";
  const mins = Math.floor(diff / 60_000);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ${mins % 60}m`;
  return `${Math.floor(hrs / 24)}d ${hrs % 24}h`;
}

export function EnabledCapabilitiesDashboard({ isDark, groupId, refreshKey }: EnabledCapabilitiesDashboardProps) {
  const { t } = useTranslation("settings");
  const [entries, setEntries] = useState<CapabilityEnabledEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState("");
  const [busyId, setBusyId] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!groupId) return;
    setLoading(true);
    setErr("");
    try {
      const resp = await api.fetchGroupCapabilityState(groupId);
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilities.enabledDashboard.failedLoad"));
        setEntries([]);
      } else {
        const groupOnly = (Array.isArray(resp.result?.enabled) ? resp.result.enabled : [])
          .filter((entry) => String(entry.scope || "").trim().toLowerCase() === "group");
        setEntries(groupOnly);
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("capabilities.enabledDashboard.failedLoad"));
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, [groupId, t]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const handleDisable = async (entry: CapabilityEnabledEntry) => {
    if (!groupId) return;
    const capId = String(entry.capability_id || "").trim();
    if (!capId) return;
    setBusyId(capId);
    setErr("");
    try {
      const resp = await api.enableGroupCapability(groupId, capId, {
        enabled: false,
        scope: "group",
        cleanup: true,
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilities.enabledDashboard.failedDisable"));
        return;
      }
      const validation = validateCapabilityToggleResult(resp.result, false);
      if (!validation.ok) {
        setErr(
          validation.reason
            ? t("capabilities.operationFailedReason", { reason: validation.reason })
            : t("capabilities.enabledDashboard.failedDisable")
        );
        return;
      }
      await load();
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("capabilities.enabledDashboard.failedDisable"));
    } finally {
      setBusyId("");
    }
  };

  if (!groupId) return null;

  return (
    <div className={cardClass(isDark)}>
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>
            {t("capabilities.enabledDashboard.title")}
          </div>
          <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
            {t("capabilities.enabledDashboard.subtitle", { count: entries.length })}
          </div>
        </div>
        <button
          type="button"
          className={`px-3 py-2 rounded-lg text-sm min-h-[40px] ${
            isDark ? "bg-slate-800 text-slate-200 hover:bg-slate-700" : "bg-white border border-gray-200 text-gray-700 hover:bg-gray-50"
          }`}
          onClick={() => void load()}
          disabled={loading}
        >
          {loading ? t("common:loading") : t("capabilities.refresh")}
        </button>
      </div>

      {err ? (
        <div className={`mt-2 text-xs ${isDark ? "text-rose-300" : "text-rose-700"}`} role="alert">{err}</div>
      ) : null}

      <div className="mt-2 space-y-2">
        {entries.length === 0 && !loading ? (
          <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
            {t("capabilities.enabledDashboard.empty")}
          </div>
        ) : null}
        {entries.map((entry) => {
          const capId = String(entry.capability_id || "");
          const expanded = expandedId === capId;
          const ttlLabel = formatTtl(entry.expires_at);

          return (
            <div
              key={capId}
              className={`rounded-lg border px-3 py-2 ${isDark ? "border-slate-800 bg-slate-900/40" : "border-gray-200 bg-white"}`}
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className={`text-sm font-medium truncate ${isDark ? "text-slate-100" : "text-gray-900"}`}>{capId}</div>
                  <div className="flex flex-wrap gap-1 mt-1">
                    {ttlLabel ? (
                      <span className={`px-1.5 py-0.5 rounded text-[10px] ${isDark ? "bg-slate-800 text-slate-300" : "bg-gray-100 text-gray-700"}`}>
                        {ttlLabel}
                      </span>
                    ) : null}
                    {entry.tool_count ? (
                      <button
                        type="button"
                        className={`px-1.5 py-0.5 rounded text-[10px] cursor-pointer ${
                          isDark ? "bg-slate-800 text-slate-300 hover:bg-slate-700" : "bg-gray-100 text-gray-700 hover:bg-gray-200"
                        }`}
                        onClick={() => setExpandedId(expanded ? null : capId)}
                      >
                        {t("capabilities.enabledDashboard.tools", { count: entry.tool_count })}
                      </button>
                    ) : null}
                  </div>
                </div>
                <button
                  type="button"
                  className={`px-2.5 py-1.5 rounded text-xs min-h-[32px] ${
                    isDark ? "bg-rose-900/40 text-rose-300" : "bg-rose-50 text-rose-700 border border-rose-200"
                  } ${busyId === capId ? "opacity-60 cursor-not-allowed" : ""}`}
                  disabled={busyId === capId || loading}
                  onClick={() => void handleDisable(entry)}
                >
                  {t("capabilities.enabledDashboard.disable")}
                </button>
              </div>
              {expanded && entry.tool_names?.length ? (
                <div className={`mt-2 pl-2 border-l-2 ${isDark ? "border-slate-700" : "border-gray-200"}`}>
                  {entry.tool_names.map((name) => (
                    <div key={name} className={`text-[11px] py-0.5 ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                      {name}
                    </div>
                  ))}
                </div>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}
