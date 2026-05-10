import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../services/api";
import { CapabilityOverviewItem } from "../types";
import { normalizeCapabilityIdList } from "../utils/capabilityAutoload";
import { Input } from "./ui/input";
import { Surface } from "./ui/surface";

interface CapabilityPickerProps {
  isDark: boolean;
  value: string[];
  onChange: (next: string[]) => void;
  active?: boolean;
  disabled?: boolean;
  label?: string;
  hint?: string;
}

const BADGE_CLASS = "bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)] border border-[var(--glass-border-subtle)]";
const CAPABILITY_PICKER_FETCH_LIMIT = 200;
const CAPABILITY_PICKER_QUERY_DEBOUNCE_MS = 250;

function firstRecommendationLine(value?: string[]) {
  return Array.isArray(value) ? String(value[0] || "").trim() : "";
}

export function CapabilityPicker({
  isDark: _isDark,
  value,
  onChange,
  active = true,
  disabled = false,
  label = "",
  hint = "",
}: CapabilityPickerProps) {
  const { t } = useTranslation("settings");
  const selected = normalizeCapabilityIdList(value);
  const selectedSet = useMemo(() => new Set(selected), [selected]);
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [rows, setRows] = useState<CapabilityOverviewItem[]>([]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebouncedQuery(String(query || "").trim());
    }, CAPABILITY_PICKER_QUERY_DEBOUNCE_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [query]);

  useEffect(() => {
    if (!active) {
      setLoading(false);
      return;
    }
    let cancelled = false;
    const run = async () => {
      setLoading(true);
      setError("");
      try {
        const resp = await api.fetchCapabilityOverview({
          includeIndexed: true,
          includeSourceInstances: false,
          limit: CAPABILITY_PICKER_FETCH_LIMIT,
          query: debouncedQuery || undefined,
        });
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
  }, [active, debouncedQuery]);

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
        ...(Array.isArray(row.use_when) ? row.use_when.map((x) => String(x || "")) : []),
        ...(Array.isArray(row.avoid_when) ? row.avoid_when.map((x) => String(x || "")) : []),
        ...(Array.isArray(row.gotchas) ? row.gotchas.map((x) => String(x || "")) : []),
        String(row.evidence_kind || ""),
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
      {label ? <label className="block text-xs font-medium mb-2 text-[var(--color-text-tertiary)]">{label}</label> : null}

      <div className="flex flex-wrap gap-1.5 mb-2">
        {selected.length === 0 ? (
          <span className="text-xs text-[var(--color-text-muted)]">{t("capabilities.noneSelected")}</span>
        ) : (
          selected.map((capId) => (
            <button
              key={capId}
              type="button"
              onClick={() => toggle(capId)}
              disabled={disabled}
              className={`px-2 py-1 rounded text-[11px] ${BADGE_CLASS} ${disabled ? "opacity-60 cursor-not-allowed" : "hover:opacity-85"}`}
              title={t("capabilities.removeFromAutoload")}
            >
              {capId}
            </button>
          ))
        )}
      </div>

      <Input
        type="text"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        disabled={disabled}
        placeholder={t("capabilities.searchPlaceholder")}
        className="rounded-lg px-3 py-2 min-h-[40px]"
      />

      <Surface
        variant="subtle"
        padding="none"
        radius="md"
        className="mt-2 max-h-56 overflow-auto border-[var(--glass-border-subtle)]"
      >
        {!active ? (
          <div className="px-3 py-3 text-xs text-[var(--color-text-tertiary)]">{t("capabilities.openToLoad", { defaultValue: "Open this section to load capabilities." })}</div>
        ) : loading ? (
          <div className="px-3 py-3 text-xs text-[var(--color-text-tertiary)]">{t("capabilities.loading")}</div>
        ) : error ? (
          <div className="px-3 py-3 text-xs text-rose-700 dark:text-rose-300">{error}</div>
        ) : candidateRows.length === 0 ? (
          <div className="px-3 py-3 text-xs text-[var(--color-text-tertiary)]">{t("capabilities.noCandidates")}</div>
        ) : (
          candidateRows.map((row) => {
            const capId = String(row.capability_id || "").trim();
            const selectedNow = selectedSet.has(capId);
            const blocked = Boolean(row.blocked_global);
            const disabledItem = disabled || blocked;
            const recommendationMeta = [
              { label: t("capabilities.useWhen"), value: firstRecommendationLine(row.use_when) },
              { label: t("capabilities.verifyWith"), value: String(row.evidence_kind || "").trim() },
              { label: t("capabilities.gotcha"), value: firstRecommendationLine(row.gotchas) },
              { label: t("capabilities.avoidWhen"), value: firstRecommendationLine(row.avoid_when) },
            ].filter((entry) => entry.value).slice(0, 2);
            return (
              <label
                key={capId}
                className={`flex items-start gap-2 px-3 py-2 border-b last:border-b-0 border-[var(--glass-border-subtle)] text-[var(--color-text-primary)] ${disabledItem ? "opacity-60" : "cursor-pointer"}`}
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
                  <div className="text-[11px] truncate text-[var(--color-text-tertiary)]">{capId}</div>
                  {String(row.description_short || "").trim() ? (
                    <div className="text-[11px] mt-0.5 text-[var(--color-text-tertiary)]">{String(row.description_short || "")}</div>
                  ) : null}
                  {recommendationMeta.length ? (
                    <div className="mt-1 space-y-0.5">
                      {recommendationMeta.map((entry) => (
                        <div key={`${capId}:${entry.label}`} className="text-[10px] leading-4 text-[var(--color-text-muted)]">
                          <span className="font-medium text-[var(--color-text-tertiary)]">{entry.label}: </span>
                          <span>{entry.value}</span>
                        </div>
                      ))}
                    </div>
                  ) : null}
                  <div className="flex flex-wrap gap-1 mt-1">
                    {row.kind ? <span className={`px-1.5 py-0.5 rounded text-[10px] ${BADGE_CLASS}`}>{row.kind}</span> : null}
                    {row.source_id ? <span className={`px-1.5 py-0.5 rounded text-[10px] ${BADGE_CLASS}`}>{row.source_id}</span> : null}
                    {row.recent_success?.success_count ? (
                      <span className={`px-1.5 py-0.5 rounded text-[10px] ${BADGE_CLASS}`}>
                        {t("capabilities.recentCount", { count: Number(row.recent_success?.success_count || 0) })}
                      </span>
                    ) : null}
                    {blocked ? (
                      <span className="px-1.5 py-0.5 rounded text-[10px] bg-rose-50 text-rose-700 border border-rose-200 dark:bg-rose-900/30 dark:text-rose-300 dark:border-rose-800">
                        {t("capabilities.blocked")}
                      </span>
                    ) : null}
                  </div>
                </div>
              </label>
            );
          })
        )}
      </Surface>

      {hint ? <div className="text-[10px] mt-1 text-[var(--color-text-muted)]">{hint}</div> : null}
    </div>
  );
}
