import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../services/api";
import { CapabilityOverviewItem } from "../types";
import { normalizeCapabilityIdList } from "../utils/capabilityAutoload";

interface CapabilityPickerProps {
  isDark: boolean;
  value: string[];
  onChange: (next: string[]) => void;
  disabled?: boolean;
  label?: string;
  hint?: string;
}

function badgeClass(isDark: boolean): string {
  return isDark ? "bg-slate-800 text-slate-300 border border-slate-700" : "bg-gray-100 text-gray-700 border border-gray-200";
}

export function CapabilityPicker({
  isDark,
  value,
  onChange,
  disabled = false,
  label = "",
  hint = "",
}: CapabilityPickerProps) {
  const { t } = useTranslation("settings");
  const selected = normalizeCapabilityIdList(value);
  const selectedSet = useMemo(() => new Set(selected), [selected]);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [rows, setRows] = useState<CapabilityOverviewItem[]>([]);

  useEffect(() => {
    let cancelled = false;
    const run = async () => {
      setLoading(true);
      setError("");
      try {
        const resp = await api.fetchCapabilityOverview({ includeIndexed: true, limit: 1200 });
        if (cancelled) return;
        if (!resp.ok) {
          setError(resp.error?.message || "Failed to load capabilities");
          setRows([]);
          return;
        }
        setRows(Array.isArray(resp.result?.items) ? resp.result.items : []);
      } catch (e) {
        if (cancelled) return;
        setError(e instanceof Error ? e.message : "Failed to load capabilities");
        setRows([]);
      } finally {
        if (!cancelled) setLoading(false);
      }
    };
    void run();
    return () => {
      cancelled = true;
    };
  }, []);

  const candidateRows = useMemo(() => {
    const q = String(query || "").trim().toLowerCase();
    const filtered = rows.filter((row) => {
      const capId = String(row.capability_id || "").trim();
      if (!capId) return false;
      const candidate = Boolean(row.autoload_candidate) || selectedSet.has(capId);
      if (!candidate) return false;
      if (!q) return true;
      const text = [
        capId,
        String(row.name || ""),
        String(row.description_short || ""),
        String(row.source_id || ""),
        ...(Array.isArray(row.tags) ? row.tags.map((x) => String(x || "")) : []),
      ]
        .join(" ")
        .toLowerCase();
      return text.includes(q);
    });
    filtered.sort((a, b) => {
      const aId = String(a.capability_id || "");
      const bId = String(b.capability_id || "");
      const aSelected = selectedSet.has(aId) ? 0 : 1;
      const bSelected = selectedSet.has(bId) ? 0 : 1;
      if (aSelected !== bSelected) return aSelected - bSelected;
      const aRecent = Number(a.recent_success?.success_count || 0);
      const bRecent = Number(b.recent_success?.success_count || 0);
      if (aRecent !== bRecent) return bRecent - aRecent;
      return String(a.name || aId).localeCompare(String(b.name || bId));
    });
    return filtered;
  }, [rows, selectedSet, query]);

  const toggle = (capabilityId: string) => {
    const capId = String(capabilityId || "").trim();
    if (!capId) return;
    if (selectedSet.has(capId)) {
      onChange(selected.filter((item) => item !== capId));
      return;
    }
    onChange([...selected, capId]);
  };

  return (
    <div>
      {label ? <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{label}</label> : null}

      <div className="flex flex-wrap gap-1.5 mb-2">
        {selected.length === 0 ? (
          <span className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("capabilities.noneSelected")}</span>
        ) : (
          selected.map((capId) => (
            <button
              key={capId}
              type="button"
              onClick={() => toggle(capId)}
              disabled={disabled}
              className={`px-2 py-1 rounded text-[11px] ${badgeClass(isDark)} ${disabled ? "opacity-60 cursor-not-allowed" : "hover:opacity-85"}`}
              title={t("capabilities.removeFromAutoload")}
            >
              {capId}
            </button>
          ))
        )}
      </div>

      <input
        type="text"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        disabled={disabled}
        placeholder={t("capabilities.searchPlaceholder")}
        className={`w-full rounded-lg border px-3 py-2 text-sm min-h-[40px] ${
          isDark ? "bg-slate-900 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"
        }`}
      />

      <div
        className={`mt-2 rounded-lg border max-h-56 overflow-auto ${isDark ? "border-slate-700 bg-slate-950/50" : "border-gray-200 bg-gray-50"}`}
      >
        {loading ? (
          <div className={`px-3 py-3 text-xs ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("capabilities.loading")}</div>
        ) : error ? (
          <div className={`px-3 py-3 text-xs ${isDark ? "text-rose-300" : "text-rose-700"}`}>{error}</div>
        ) : candidateRows.length === 0 ? (
          <div className={`px-3 py-3 text-xs ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("capabilities.noCandidates")}</div>
        ) : (
          candidateRows.map((row) => {
            const capId = String(row.capability_id || "").trim();
            const selectedNow = selectedSet.has(capId);
            const blocked = Boolean(row.blocked_global);
            const disabledItem = disabled || blocked;
            return (
              <label
                key={capId}
                className={`flex items-start gap-2 px-3 py-2 border-b last:border-b-0 ${
                  isDark ? "border-slate-800 text-slate-200" : "border-gray-200 text-gray-800"
                } ${disabledItem ? "opacity-60" : "cursor-pointer"}`}
              >
                <input
                  type="checkbox"
                  checked={selectedNow}
                  disabled={disabledItem}
                  onChange={() => toggle(capId)}
                  className="mt-0.5"
                />
                <div className="min-w-0">
                  <div className="text-xs font-medium truncate">{String(row.name || capId)}</div>
                  <div className={`text-[11px] truncate ${isDark ? "text-slate-400" : "text-gray-600"}`}>{capId}</div>
                  {String(row.description_short || "").trim() ? (
                    <div className={`text-[11px] mt-0.5 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{String(row.description_short || "")}</div>
                  ) : null}
                  <div className="flex flex-wrap gap-1 mt-1">
                    {row.kind ? <span className={`px-1.5 py-0.5 rounded text-[10px] ${badgeClass(isDark)}`}>{row.kind}</span> : null}
                    {row.source_id ? <span className={`px-1.5 py-0.5 rounded text-[10px] ${badgeClass(isDark)}`}>{row.source_id}</span> : null}
                    {row.recent_success?.success_count ? (
                      <span className={`px-1.5 py-0.5 rounded text-[10px] ${badgeClass(isDark)}`}>
                        {t("capabilities.recentCount", { count: Number(row.recent_success?.success_count || 0) })}
                      </span>
                    ) : null}
                    {blocked ? (
                      <span className={`px-1.5 py-0.5 rounded text-[10px] ${isDark ? "bg-rose-900/30 text-rose-300 border border-rose-800" : "bg-rose-50 text-rose-700 border border-rose-200"}`}>
                        {t("capabilities.blocked")}
                      </span>
                    ) : null}
                  </div>
                </div>
              </label>
            );
          })
        )}
      </div>

      {hint ? <div className={`text-[10px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{hint}</div> : null}
    </div>
  );
}

