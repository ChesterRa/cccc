import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import { CapabilityBlockEntry, CapabilityOverviewItem, CapabilitySourceState } from "../../../types";
import { cardClass } from "./types";

interface CapabilitiesTabProps {
  isDark: boolean;
  isActive: boolean;
  groupId?: string;
}

type SourceVisibility = "all" | "enabled" | "disabled";
type LibraryKindFilter = "all" | "pack" | "mcp" | "skill";
type LibraryPolicyFilter = "all" | "actionable" | "blocked" | "indexed";

const SOURCE_PREVIEW_LIMIT = 8;
const LIBRARY_PAGE_SIZE_OPTIONS = [20, 40, 80];
const SOURCE_PRIORITY: Record<string, number> = {
  cccc_builtin: 0,
  mcp_registry_official: 1,
  anthropic_skills: 2,
  github_skills_curated: 3,
  openclaw_skills_remote: 4,
  clawskills_remote: 5,
  clawhub_remote: 6,
  skillsmp_remote: 7,
};

export function CapabilitiesTab({ isDark, isActive, groupId }: CapabilitiesTabProps) {
  const { t } = useTranslation("settings");
  const [loading, setLoading] = useState(false);
  const [busyCapId, setBusyCapId] = useState("");
  const [err, setErr] = useState("");
  const [query, setQuery] = useState("");
  const [sourceQuery, setSourceQuery] = useState("");
  const [sourceVisibility, setSourceVisibility] = useState<SourceVisibility>("all");
  const [showAllSources, setShowAllSources] = useState(false);
  const [libraryKind, setLibraryKind] = useState<LibraryKindFilter>("all");
  const [libraryPolicy, setLibraryPolicy] = useState<LibraryPolicyFilter>("all");
  const [librarySource, setLibrarySource] = useState("all");
  const [libraryPageSize, setLibraryPageSize] = useState(40);
  const [libraryPage, setLibraryPage] = useState(1);
  const [revision, setRevision] = useState("");
  const [items, setItems] = useState<CapabilityOverviewItem[]>([]);
  const [sources, setSources] = useState<Record<string, CapabilitySourceState>>({});
  const [blocked, setBlocked] = useState<CapabilityBlockEntry[]>([]);
  const [allowlistSources, setAllowlistSources] = useState<Array<{ source_id: string; enabled: boolean; rationale?: string }>>([]);

  const load = async () => {
    if (!isActive) return;
    setLoading(true);
    setErr("");
    try {
      const [overviewResp, allowlistResp] = await Promise.all([
        api.fetchCapabilityOverview({ includeIndexed: true, limit: 1200 }),
        api.fetchCapabilityAllowlist(),
      ]);
      if (!overviewResp.ok) {
        setErr(overviewResp.error?.message || t("capabilities.failedLoad"));
        setItems([]);
        setSources({});
        setBlocked([]);
      } else {
        setItems(Array.isArray(overviewResp.result?.items) ? overviewResp.result.items : []);
        setSources(
          overviewResp.result?.sources && typeof overviewResp.result.sources === "object"
            ? overviewResp.result.sources
            : {}
        );
        setBlocked(
          Array.isArray(overviewResp.result?.blocked_capabilities)
            ? overviewResp.result.blocked_capabilities
            : []
        );
        setRevision(String(overviewResp.result?.allowlist_revision || ""));
      }
      if (allowlistResp.ok) {
        const allowRevision = String(allowlistResp.result?.revision || revision);
        setRevision(allowRevision);
        const effective = allowlistResp.result?.effective && typeof allowlistResp.result.effective === "object"
          ? allowlistResp.result.effective
          : {};
        const effectiveSources = Array.isArray((effective as { sources?: unknown[] }).sources)
          ? (effective as { sources: unknown[] }).sources
          : [];
        const nextSources = effectiveSources
          .filter((row) => row && typeof row === "object")
          .map((row) => {
            const obj = row as Record<string, unknown>;
            return {
              source_id: String(obj.source_id || "").trim(),
              enabled: Boolean(obj.enabled),
              rationale: String(obj.rationale || ""),
            };
          })
          .filter((row) => row.source_id.length > 0);
        setAllowlistSources(nextSources);
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("capabilities.failedLoad"));
      setItems([]);
      setSources({});
      setBlocked([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!isActive) return;
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isActive]);

  const sourceRationaleMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const row of allowlistSources) {
      const sid = String(row.source_id || "").trim();
      if (!sid) continue;
      map.set(sid, String(row.rationale || "").trim());
    }
    return map;
  }, [allowlistSources]);

  const sourceRows = useMemo(() => {
    return Object.values(sources || {}).sort((a, b) => {
      const aEnabled = a.enabled ? 0 : 1;
      const bEnabled = b.enabled ? 0 : 1;
      if (aEnabled !== bEnabled) return aEnabled - bEnabled;
      const aPriority = SOURCE_PRIORITY[String(a.source_id || "").trim()] ?? 99;
      const bPriority = SOURCE_PRIORITY[String(b.source_id || "").trim()] ?? 99;
      if (aPriority !== bPriority) return aPriority - bPriority;
      const aCount = Number(a.record_count || 0);
      const bCount = Number(b.record_count || 0);
      if (aCount !== bCount) return bCount - aCount;
      return String(a.source_id || "").localeCompare(String(b.source_id || ""));
    });
  }, [sources]);

  const sourceSummary = useMemo(() => {
    const total = sourceRows.length;
    const enabled = sourceRows.filter((row) => Boolean(row.enabled)).length;
    return {
      total,
      enabled,
      disabled: Math.max(0, total - enabled),
    };
  }, [sourceRows]);

  const filteredSources = useMemo(() => {
    const q = String(sourceQuery || "").trim().toLowerCase();
    return sourceRows.filter((row) => {
      const enabled = Boolean(row.enabled);
      if (sourceVisibility === "enabled" && !enabled) return false;
      if (sourceVisibility === "disabled" && enabled) return false;
      if (!q) return true;
      const text = [
        String(row.source_id || ""),
        String(row.source_level || ""),
        String(row.sync_state || ""),
        String(sourceRationaleMap.get(String(row.source_id || "").trim()) || ""),
        String(row.rationale || ""),
      ]
        .join(" ")
        .toLowerCase();
      return text.includes(q);
    });
  }, [sourceRows, sourceQuery, sourceVisibility, sourceRationaleMap]);

  const sourceRowsVisible = useMemo(() => {
    if (showAllSources) return filteredSources;
    return filteredSources.slice(0, SOURCE_PREVIEW_LIMIT);
  }, [filteredSources, showAllSources]);

  const librarySourceOptions = useMemo(() => {
    const out = new Set<string>();
    for (const row of items) {
      const sid = String(row.source_id || "").trim();
      if (sid) out.add(sid);
    }
    return Array.from(out).sort((a, b) => {
      const aPriority = SOURCE_PRIORITY[a] ?? 99;
      const bPriority = SOURCE_PRIORITY[b] ?? 99;
      if (aPriority !== bPriority) return aPriority - bPriority;
      return a.localeCompare(b);
    });
  }, [items]);

  const filteredLibrary = useMemo(() => {
    const q = String(query || "").trim().toLowerCase();
    const rows = items.filter((row) => {
      const capId = String(row.capability_id || "").trim();
      const kind = String(row.kind || "").trim().toLowerCase();
      const blockedNow = Boolean(row.blocked_global);
      const policyLevel = String(row.policy_level || "").trim().toLowerCase();
      const policyVisible = policyLevel !== "indexed";

      if (libraryKind === "pack" && kind !== "pack") return false;
      if (libraryKind === "mcp" && kind !== "mcp_toolpack") return false;
      if (libraryKind === "skill" && kind !== "skill") return false;

      if (libraryPolicy === "actionable" && (!policyVisible || blockedNow)) return false;
      if (libraryPolicy === "blocked" && !blockedNow) return false;
      if (libraryPolicy === "indexed" && policyLevel !== "indexed") return false;

      if (librarySource !== "all" && String(row.source_id || "").trim() !== librarySource) return false;

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
    rows.sort((a, b) => {
      const aBlocked = a.blocked_global ? 1 : 0;
      const bBlocked = b.blocked_global ? 1 : 0;
      if (aBlocked !== bBlocked) return aBlocked - bBlocked;
      const aPolicy = String(a.policy_level || "").toLowerCase() === "indexed" ? 1 : 0;
      const bPolicy = String(b.policy_level || "").toLowerCase() === "indexed" ? 1 : 0;
      if (aPolicy !== bPolicy) return aPolicy - bPolicy;
      const aRecent = Number(a.recent_success?.success_count || 0);
      const bRecent = Number(b.recent_success?.success_count || 0);
      if (aRecent !== bRecent) return bRecent - aRecent;
      return String(a.name || a.capability_id || "").localeCompare(String(b.name || b.capability_id || ""));
    });
    return rows;
  }, [items, query, libraryKind, libraryPolicy, librarySource]);

  const libraryTotalPages = useMemo(
    () => Math.max(1, Math.ceil(Math.max(1, filteredLibrary.length) / Math.max(1, libraryPageSize))),
    [filteredLibrary.length, libraryPageSize]
  );

  useEffect(() => {
    setLibraryPage(1);
  }, [query, libraryKind, libraryPolicy, librarySource, libraryPageSize]);

  useEffect(() => {
    setLibraryPage((prev) => {
      if (prev <= libraryTotalPages) return prev;
      return libraryTotalPages;
    });
  }, [libraryTotalPages]);

  const pagedLibrary = useMemo(() => {
    const safePage = Math.max(1, Math.min(libraryPage, libraryTotalPages));
    const start = (safePage - 1) * libraryPageSize;
    return filteredLibrary.slice(start, start + libraryPageSize);
  }, [filteredLibrary, libraryPage, libraryPageSize, libraryTotalPages]);

  const libraryRange = useMemo(() => {
    if (!filteredLibrary.length) return { from: 0, to: 0 };
    const safePage = Math.max(1, Math.min(libraryPage, libraryTotalPages));
    const from = (safePage - 1) * libraryPageSize + 1;
    const to = from + pagedLibrary.length - 1;
    return { from, to };
  }, [filteredLibrary.length, libraryPage, libraryPageSize, libraryTotalPages, pagedLibrary.length]);

  const toggleBlock = async (row: CapabilityOverviewItem, nextBlocked: boolean) => {
    const capabilityId = String(row.capability_id || "").trim();
    if (!capabilityId) return;
    if (!groupId) {
      setErr(t("capabilities.requireGroup"));
      return;
    }
    let reason = "";
    if (nextBlocked) {
      reason = String(window.prompt(t("capabilities.blockReasonPrompt"), row.blocked_reason || "") || "").trim();
    }
    setBusyCapId(capabilityId);
    setErr("");
    try {
      const resp = await api.blockCapabilityGlobal(groupId, capabilityId, nextBlocked, reason);
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilities.failedBlock"));
        return;
      }
      await load();
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("capabilities.failedBlock"));
    } finally {
      setBusyCapId("");
    }
  };

  const toggleSource = async (sourceId: string, nextEnabled: boolean) => {
    const sid = String(sourceId || "").trim();
    if (!sid) return;
    setBusyCapId(`source:${sid}`);
    setErr("");
    try {
      const baseline = allowlistSources.length > 0
        ? allowlistSources
        : sourceRows.map((row) => ({
            source_id: String(row.source_id || "").trim(),
            enabled: Boolean(row.enabled),
            rationale: "",
          }));
      const mergedById = new Map<string, { source_id: string; enabled: boolean; rationale?: string }>();
      for (const row of baseline) {
        const id = String(row.source_id || "").trim();
        if (!id) continue;
        mergedById.set(id, { source_id: id, enabled: Boolean(row.enabled), rationale: String(row.rationale || "") });
      }
      for (const row of sourceRows) {
        const id = String(row.source_id || "").trim();
        if (!id || mergedById.has(id)) continue;
        mergedById.set(id, { source_id: id, enabled: Boolean(row.enabled), rationale: "" });
      }
      const current = Array.from(mergedById.values());
      const idx = current.findIndex((row) => String(row.source_id || "").trim() === sid);
      if (idx >= 0) {
        current[idx] = {
          ...current[idx],
          enabled: nextEnabled,
        };
      } else {
        current.push({
          source_id: sid,
          enabled: nextEnabled,
          rationale: "",
        });
      }
      const patchSources = current.map((row) => ({
        source_id: String(row.source_id || "").trim(),
        enabled: Boolean(row.enabled),
        rationale: String(row.rationale || ""),
      }));
      const resp = await api.updateCapabilityAllowlist({
        patch: { sources: patchSources },
        expectedRevision: String(revision || "").trim() || undefined,
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilities.failedSourceToggle"));
        return;
      }
      await load();
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("capabilities.failedSourceToggle"));
    } finally {
      setBusyCapId("");
    }
  };

  return (
    <div className="space-y-4">
      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("capabilities.title")}</div>
            <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("capabilities.subtitle")}</div>
          </div>
          <button
            type="button"
            className={`px-3 py-2 rounded-lg text-sm min-h-[40px] ${isDark ? "bg-slate-800 text-slate-200 hover:bg-slate-700" : "bg-white border border-gray-200 text-gray-700 hover:bg-gray-50"}`}
            onClick={() => void load()}
            disabled={loading}
          >
            {loading ? t("common:loading") : t("capabilities.refresh")}
          </button>
        </div>
        <div className={`text-[11px] mt-2 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("capabilities.revision")}: <code>{revision || "-"}</code>
        </div>
        {err ? (
          <div className={`mt-2 text-xs ${isDark ? "text-rose-300" : "text-rose-700"}`} role="alert">
            {err}
          </div>
        ) : null}
      </div>

      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("capabilities.sourcesTitle")}</div>
        <div className={`text-xs mt-1 mb-2 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("capabilities.sourcesHint")}</div>
        <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("capabilities.sourcesSummary", sourceSummary)}
        </div>
        <div className="mt-2 grid grid-cols-1 md:grid-cols-[minmax(0,1fr)_auto] gap-2 items-center">
          <input
            value={sourceQuery}
            onChange={(e) => setSourceQuery(e.target.value)}
            placeholder={t("capabilities.sourceSearchPlaceholder")}
            className={`w-full rounded-lg border px-3 py-2 text-sm min-h-[40px] ${
              isDark ? "bg-slate-900 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"
            }`}
          />
          <div className="inline-flex rounded-lg border overflow-hidden">
            <button
              type="button"
              onClick={() => setSourceVisibility("all")}
              className={`px-2.5 py-2 text-xs min-h-[40px] ${
                sourceVisibility === "all"
                  ? isDark
                    ? "bg-slate-700 text-slate-100"
                    : "bg-gray-100 text-gray-900"
                  : isDark
                    ? "bg-slate-900 text-slate-300"
                    : "bg-white text-gray-700"
              }`}
            >
              {t("capabilities.sourcesVisibilityAll")}
            </button>
            <button
              type="button"
              onClick={() => setSourceVisibility("enabled")}
              className={`px-2.5 py-2 text-xs min-h-[40px] border-l ${
                sourceVisibility === "enabled"
                  ? isDark
                    ? "bg-slate-700 text-slate-100 border-slate-600"
                    : "bg-gray-100 text-gray-900 border-gray-200"
                  : isDark
                    ? "bg-slate-900 text-slate-300 border-slate-700"
                    : "bg-white text-gray-700 border-gray-200"
              }`}
            >
              {t("capabilities.sourcesVisibilityEnabled")}
            </button>
            <button
              type="button"
              onClick={() => setSourceVisibility("disabled")}
              className={`px-2.5 py-2 text-xs min-h-[40px] border-l ${
                sourceVisibility === "disabled"
                  ? isDark
                    ? "bg-slate-700 text-slate-100 border-slate-600"
                    : "bg-gray-100 text-gray-900 border-gray-200"
                  : isDark
                    ? "bg-slate-900 text-slate-300 border-slate-700"
                    : "bg-white text-gray-700 border-gray-200"
              }`}
            >
              {t("capabilities.sourcesVisibilityDisabled")}
            </button>
          </div>
        </div>
        <div className="space-y-2">
          {sourceRowsVisible.map((row) => (
            <div
              key={row.source_id}
              className={`rounded-lg border px-3 py-2 ${isDark ? "border-slate-800 bg-slate-900/40" : "border-gray-200 bg-white"}`}
            >
              <div className="flex items-center justify-between gap-3">
                <code className="text-xs">{row.source_id}</code>
                <div className="flex items-center gap-2">
                  <div className={`text-[11px] ${row.enabled ? (isDark ? "text-emerald-300" : "text-emerald-700") : isDark ? "text-slate-500" : "text-gray-500"}`}>
                    {row.enabled ? t("capabilities.sourceEnabled") : t("capabilities.sourceDisabled")}
                  </div>
                  <button
                    type="button"
                    className={`px-2.5 py-1 rounded text-[11px] min-h-[30px] ${
                      row.enabled
                        ? isDark
                          ? "bg-rose-900/40 text-rose-300"
                          : "bg-rose-50 text-rose-700 border border-rose-200"
                        : isDark
                          ? "bg-emerald-900/40 text-emerald-300"
                          : "bg-emerald-50 text-emerald-700 border border-emerald-200"
                    } ${(busyCapId === `source:${row.source_id}` || loading) ? "opacity-60 cursor-not-allowed" : ""}`}
                    disabled={busyCapId === `source:${row.source_id}` || loading}
                    onClick={() => void toggleSource(row.source_id, !row.enabled)}
                  >
                    {row.enabled ? t("capabilities.sourceDisableAction") : t("capabilities.sourceEnableAction")}
                  </button>
                </div>
              </div>
              <div className={`text-[11px] mt-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                {t("capabilities.sourceMeta", {
                  level: String(row.source_level || "indexed"),
                  sync: String(row.sync_state || "never"),
                  count: Number(row.record_count || 0),
                })}
              </div>
              {String(sourceRationaleMap.get(String(row.source_id || "").trim()) || String(row.rationale || "")).trim() ? (
                <div className={`text-[11px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  {t("capabilities.sourceRationale", {
                    text: String(sourceRationaleMap.get(String(row.source_id || "").trim()) || String(row.rationale || "")).trim(),
                  })}
                </div>
              ) : null}
              {String(row.error || "").trim() ? (
                <div className={`text-[11px] mt-1 ${isDark ? "text-rose-300" : "text-rose-700"}`}>{String(row.error || "")}</div>
              ) : null}
            </div>
          ))}
          {filteredSources.length > SOURCE_PREVIEW_LIMIT ? (
            <button
              type="button"
              className={`w-full rounded-lg border px-3 py-2 text-xs min-h-[36px] ${
                isDark ? "border-slate-700 text-slate-300 hover:bg-slate-800/50" : "border-gray-200 text-gray-700 hover:bg-gray-50"
              }`}
              onClick={() => setShowAllSources((v) => !v)}
            >
              {showAllSources
                ? t("capabilities.showLessSources")
                : t("capabilities.showMoreSources", { count: filteredSources.length - SOURCE_PREVIEW_LIMIT })}
            </button>
          ) : null}
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("capabilities.libraryTitle")}</div>
        <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("capabilities.libraryHint")}</div>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("capabilities.searchPlaceholder")}
          className={`w-full mt-2 rounded-lg border px-3 py-2 text-sm min-h-[40px] ${
            isDark ? "bg-slate-900 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"
          }`}
        />
        <div className="mt-2 grid grid-cols-1 md:grid-cols-4 gap-2">
          <select
            value={libraryKind}
            onChange={(e) => setLibraryKind(e.target.value as LibraryKindFilter)}
            className={`rounded-lg border px-2 py-2 text-xs min-h-[40px] ${
              isDark ? "bg-slate-900 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"
            }`}
          >
            <option value="all">{t("capabilities.filterKindAll")}</option>
            <option value="pack">{t("capabilities.filterKindPack")}</option>
            <option value="mcp">{t("capabilities.filterKindMcp")}</option>
            <option value="skill">{t("capabilities.filterKindSkill")}</option>
          </select>
          <select
            value={libraryPolicy}
            onChange={(e) => setLibraryPolicy(e.target.value as LibraryPolicyFilter)}
            className={`rounded-lg border px-2 py-2 text-xs min-h-[40px] ${
              isDark ? "bg-slate-900 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"
            }`}
          >
            <option value="all">{t("capabilities.filterPolicyAll")}</option>
            <option value="actionable">{t("capabilities.filterPolicyActionable")}</option>
            <option value="blocked">{t("capabilities.filterPolicyBlocked")}</option>
            <option value="indexed">{t("capabilities.filterPolicyIndexed")}</option>
          </select>
          <select
            value={librarySource}
            onChange={(e) => setLibrarySource(e.target.value)}
            className={`rounded-lg border px-2 py-2 text-xs min-h-[40px] ${
              isDark ? "bg-slate-900 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"
            }`}
          >
            <option value="all">{t("capabilities.filterSourceAll")}</option>
            {librarySourceOptions.map((sid) => (
              <option key={sid} value={sid}>
                {sid}
              </option>
            ))}
          </select>
          <div className="grid grid-cols-[auto_minmax(0,1fr)] gap-2 items-center">
            <label className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("capabilities.pageSize")}</label>
            <select
              value={libraryPageSize}
              onChange={(e) => setLibraryPageSize(Number(e.target.value) || 40)}
              className={`rounded-lg border px-2 py-2 text-xs min-h-[40px] ${
                isDark ? "bg-slate-900 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"
              }`}
            >
              {LIBRARY_PAGE_SIZE_OPTIONS.map((size) => (
                <option key={size} value={size}>
                  {size}
                </option>
              ))}
            </select>
          </div>
        </div>
        <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("capabilities.resultsSummary", { count: filteredLibrary.length })} ·{" "}
          {t("capabilities.showingRange", { from: libraryRange.from, to: libraryRange.to })}
        </div>
        <div className="mt-2 max-h-[420px] overflow-auto space-y-2">
          {pagedLibrary.map((row) => {
            const capId = String(row.capability_id || "");
            const blockedNow = Boolean(row.blocked_global);
            return (
              <div
                key={capId}
                className={`rounded-lg border px-3 py-2 ${isDark ? "border-slate-800 bg-slate-900/40" : "border-gray-200 bg-white"}`}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className={`text-sm font-medium truncate ${isDark ? "text-slate-100" : "text-gray-900"}`}>{String(row.name || capId)}</div>
                    <div className={`text-[11px] truncate ${isDark ? "text-slate-400" : "text-gray-600"}`}>{capId}</div>
                    {String(row.description_short || "").trim() ? (
                      <div className={`text-[11px] mt-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{String(row.description_short || "")}</div>
                    ) : null}
                    <div className="flex flex-wrap gap-1 mt-1">
                      {row.kind ? <span className={`px-1.5 py-0.5 rounded text-[10px] ${isDark ? "bg-slate-800 text-slate-300" : "bg-gray-100 text-gray-700"}`}>{row.kind}</span> : null}
                      {row.source_id ? <span className={`px-1.5 py-0.5 rounded text-[10px] ${isDark ? "bg-slate-800 text-slate-300" : "bg-gray-100 text-gray-700"}`}>{row.source_id}</span> : null}
                      {row.policy_level ? <span className={`px-1.5 py-0.5 rounded text-[10px] ${isDark ? "bg-slate-800 text-slate-300" : "bg-gray-100 text-gray-700"}`}>{row.policy_level}</span> : null}
                      {row.recent_success?.success_count ? (
                        <span className={`px-1.5 py-0.5 rounded text-[10px] ${isDark ? "bg-emerald-900/40 text-emerald-300" : "bg-emerald-50 text-emerald-700"}`}>
                          {t("capabilities.recentCount", { count: Number(row.recent_success?.success_count || 0) })}
                        </span>
                      ) : null}
                      {blockedNow ? (
                        <span className={`px-1.5 py-0.5 rounded text-[10px] ${isDark ? "bg-rose-900/40 text-rose-300" : "bg-rose-50 text-rose-700"}`}>
                          {t("capabilities.blocked")}
                        </span>
                      ) : null}
                    </div>
                  </div>
                  <button
                    type="button"
                    className={`px-2.5 py-1.5 rounded text-xs min-h-[32px] ${
                      blockedNow
                        ? isDark
                          ? "bg-emerald-900/40 text-emerald-300"
                          : "bg-emerald-50 text-emerald-700 border border-emerald-200"
                        : isDark
                          ? "bg-rose-900/40 text-rose-300"
                          : "bg-rose-50 text-rose-700 border border-rose-200"
                    } ${busyCapId === capId ? "opacity-60 cursor-not-allowed" : ""}`}
                    disabled={busyCapId === capId || !groupId}
                    onClick={() => void toggleBlock(row, !blockedNow)}
                    title={!groupId ? t("capabilities.requireGroup") : ""}
                  >
                    {blockedNow ? t("capabilities.unblock") : t("capabilities.block")}
                  </button>
                </div>
              </div>
            );
          })}
          {pagedLibrary.length === 0 ? (
            <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("capabilities.noLibraryMatches")}</div>
          ) : null}
        </div>
        <div className="mt-2 flex items-center justify-between gap-2">
          <button
            type="button"
            className={`px-3 py-1.5 rounded text-xs min-h-[34px] ${
              isDark ? "bg-slate-800 text-slate-200 disabled:opacity-50" : "bg-gray-100 text-gray-700 disabled:opacity-50"
            }`}
            disabled={libraryPage <= 1}
            onClick={() => setLibraryPage((p) => Math.max(1, p - 1))}
          >
            {t("capabilities.pagePrev")}
          </button>
          <div className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
            {t("capabilities.pageLabel", { page: libraryPage, total: libraryTotalPages })}
          </div>
          <button
            type="button"
            className={`px-3 py-1.5 rounded text-xs min-h-[34px] ${
              isDark ? "bg-slate-800 text-slate-200 disabled:opacity-50" : "bg-gray-100 text-gray-700 disabled:opacity-50"
            }`}
            disabled={libraryPage >= libraryTotalPages}
            onClick={() => setLibraryPage((p) => Math.min(libraryTotalPages, p + 1))}
          >
            {t("capabilities.pageNext")}
          </button>
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("capabilities.blockedListTitle")}</div>
        <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("capabilities.blockedListHint")}</div>
        <div className="mt-2 space-y-2">
          {blocked.length === 0 ? (
            <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("capabilities.noBlocked")}</div>
          ) : (
            blocked.map((row) => (
              <div key={String(row.capability_id || "")} className={`rounded-lg border px-3 py-2 ${isDark ? "border-slate-800 bg-slate-900/40" : "border-gray-200 bg-white"}`}>
                <div className="flex items-center justify-between gap-2">
                  <code className="text-xs">{String(row.capability_id || "")}</code>
                  <button
                    type="button"
                    className={`px-2.5 py-1 rounded text-xs min-h-[30px] ${isDark ? "bg-emerald-900/40 text-emerald-300" : "bg-emerald-50 text-emerald-700 border border-emerald-200"}`}
                    disabled={busyCapId === String(row.capability_id || "") || !groupId}
                    onClick={() =>
                      void toggleBlock(
                        { capability_id: String(row.capability_id || ""), blocked_global: true },
                        false
                      )
                    }
                  >
                    {t("capabilities.unblock")}
                  </button>
                </div>
                {String(row.reason || "").trim() ? (
                  <div className={`text-[11px] mt-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{String(row.reason || "")}</div>
                ) : null}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
