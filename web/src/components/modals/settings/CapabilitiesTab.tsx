import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import { CapabilityBlockEntry, CapabilityOverviewItem, CapabilityReadinessPreview, CapabilitySourceState } from "../../../types";
import { cardClass } from "./types";

interface CapabilitiesTabProps {
  isDark: boolean;
  isActive: boolean;
}

type SourceVisibility = "all" | "enabled" | "disabled";
type RegistryKindFilter = "all" | "pack" | "mcp" | "skill";
type RegistryPolicyFilter = "all" | "actionable" | "blocked" | "indexed";
type ExternalCapabilitySafetyMode = "normal" | "conservative";

const SOURCE_PREVIEW_LIMIT = 8;
const REGISTRY_PAGE_SIZE_OPTIONS = [20, 40, 80];
const SOURCE_PRIORITY: Record<string, number> = {
  cccc_builtin: 0,
  mcp_registry_official: 1,
  anthropic_skills: 2,
  github_skills_curated: 3,
  openclaw_skills_remote: 4,
  clawskills_remote: 5,
  clawhub_remote: 6,
  skillsmp_remote: 7,
  manual_import: 8,
};
const EXTERNAL_SOURCE_IDS = [
  "manual_import",
  "mcp_registry_official",
  "anthropic_skills",
  "github_skills_curated",
  "skillsmp_remote",
  "clawhub_remote",
  "openclaw_skills_remote",
  "clawskills_remote",
] as const;

function normalizeExternalCapabilitySafetyMode(value: unknown): ExternalCapabilitySafetyMode {
  return String(value || "").trim().toLowerCase() === "conservative" ? "conservative" : "normal";
}

function normalizeReadinessPreview(value: unknown): CapabilityReadinessPreview | null {
  return value && typeof value === "object" ? (value as CapabilityReadinessPreview) : null;
}

export function CapabilitiesTab({ isDark, isActive }: CapabilitiesTabProps) {
  const { t } = useTranslation("settings");
  const [loading, setLoading] = useState(false);
  const [busyKey, setBusyKey] = useState("");
  const [err, setErr] = useState("");
  const [query, setQuery] = useState("");
  const [sourceQuery, setSourceQuery] = useState("");
  const [sourceVisibility, setSourceVisibility] = useState<SourceVisibility>("all");
  const [showAllSources, setShowAllSources] = useState(false);
  const [registryKind, setRegistryKind] = useState<RegistryKindFilter>("all");
  const [registryPolicy, setRegistryPolicy] = useState<RegistryPolicyFilter>("all");
  const [registrySource, setRegistrySource] = useState("all");
  const [registryPageSize, setRegistryPageSize] = useState(40);
  const [registryPage, setRegistryPage] = useState(1);
  const [items, setItems] = useState<CapabilityOverviewItem[]>([]);
  const [sources, setSources] = useState<Record<string, CapabilitySourceState>>({});
  const [blocked, setBlocked] = useState<CapabilityBlockEntry[]>([]);
  const [allowlistSources, setAllowlistSources] = useState<Array<{ source_id: string; enabled: boolean; rationale?: string }>>([]);
  const [externalSafetyMode, setExternalSafetyMode] = useState<ExternalCapabilitySafetyMode>("normal");

  const load = useCallback(async () => {
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
      }
      if (allowlistResp.ok) {
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
        setExternalSafetyMode(
          normalizeExternalCapabilitySafetyMode(allowlistResp.result?.external_capability_safety_mode)
        );
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("capabilities.failedLoad"));
      setItems([]);
      setSources({});
      setBlocked([]);
    } finally {
      setLoading(false);
    }
  }, [isActive, t]);

  useEffect(() => {
    if (!isActive) return;
    void load();
  }, [isActive, load]);

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

  const levelDistribution = useMemo(() => {
    const counts: Record<string, number> = { indexed: 0, mounted: 0, pinned: 0 };
    for (const row of sourceRows) {
      const level = String(row.source_level || "indexed").toLowerCase();
      if (level in counts) counts[level]++;
      else counts.indexed++;
    }
    return counts;
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

  const registrySourceOptions = useMemo(() => {
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

  const readinessBadgeClass = (status: string) => {
    if (status === "blocked") {
      return isDark ? "bg-rose-900/40 text-rose-300" : "bg-rose-50 text-rose-700";
    }
    if (status === "enableable") {
      return isDark ? "bg-emerald-900/40 text-emerald-300" : "bg-emerald-50 text-emerald-700";
    }
    return isDark ? "bg-amber-900/40 text-amber-300" : "bg-amber-50 text-amber-700";
  };

  const renderReadinessPreview = (preview: CapabilityReadinessPreview | null) => {
    if (!preview) return null;
    const status = String(preview.preview_status || "").trim().toLowerCase() || "needs_inspect";
    const nextStep = String(preview.next_step || "").trim();
    const missingEnv = Array.isArray(preview.missing_env)
      ? preview.missing_env.map((x) => String(x || "").trim()).filter(Boolean)
      : [];
    const blockedBySafetyMode =
      String(preview.policy_source || "").trim() === "external_capability_safety_mode" &&
      String(preview.policy_mode || "").trim() === "conservative";
    const blockReason = String(preview.enable_block_reason || "").trim();
    const statusLabel = t(`capabilities.readiness.status.${status}`, {
      defaultValue: status.replace(/_/g, " "),
    });
    const nextLabel = nextStep
      ? t(`capabilities.readiness.next.${nextStep}`, { defaultValue: nextStep.replace(/_/g, " ") })
      : "";
    const reasonLabel = blockedBySafetyMode
      ? t("capabilities.readiness.blockedBySafetyMode")
      : blockReason
        ? t(`capabilities.readiness.reason.${blockReason}`, { defaultValue: blockReason.replace(/_/g, " ") })
        : "";

    return (
      <div className={`mt-2 rounded-md border px-2 py-1.5 ${isDark ? "border-slate-800 bg-slate-950/60" : "border-gray-200 bg-gray-50"}`}>
        <div className="flex flex-wrap items-center gap-1.5">
          <span className={`px-1.5 py-0.5 rounded text-[10px] ${readinessBadgeClass(status)}`}>{statusLabel}</span>
          {reasonLabel ? (
            <span className={`text-[11px] ${isDark ? "text-slate-300" : "text-gray-700"}`}>{reasonLabel}</span>
          ) : null}
        </div>
        {nextLabel ? (
          <div className={`text-[11px] mt-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>
            {t("capabilities.readiness.nextLabel")}: {nextLabel}
          </div>
        ) : null}
        {missingEnv.length ? (
          <div className={`text-[11px] mt-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>
            {t("capabilities.readiness.missingEnv", { names: missingEnv.join(", ") })}
          </div>
        ) : null}
      </div>
    );
  };

  const filteredRegistry = useMemo(() => {
    const q = String(query || "").trim().toLowerCase();
    const rows = items.filter((row) => {
      const capId = String(row.capability_id || "").trim();
      const kind = String(row.kind || "").trim().toLowerCase();
      const blockedNow = Boolean(row.blocked_global);
      const policyLevel = String(row.policy_level || "").trim().toLowerCase();
      const policyVisible = policyLevel !== "indexed";
      const readinessPreview = normalizeReadinessPreview(row.readiness_preview);
      const previewStatus = String(readinessPreview?.preview_status || "").trim().toLowerCase();
      const actionableNow = previewStatus ? previewStatus === "enableable" : (policyVisible && !blockedNow);
      const blockedByReadiness = blockedNow || previewStatus === "blocked";

      if (registryKind === "pack" && kind !== "pack") return false;
      if (registryKind === "mcp" && kind !== "mcp_toolpack") return false;
      if (registryKind === "skill" && kind !== "skill") return false;

      if (registryPolicy === "actionable" && !actionableNow) return false;
      if (registryPolicy === "blocked" && !blockedByReadiness) return false;
      if (registryPolicy === "indexed" && policyLevel !== "indexed") return false;

      if (registrySource !== "all" && String(row.source_id || "").trim() !== registrySource) return false;

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
      const aBlocked = (a.blocked_global || String(normalizeReadinessPreview(a.readiness_preview)?.preview_status || "").trim().toLowerCase() === "blocked") ? 1 : 0;
      const bBlocked = (b.blocked_global || String(normalizeReadinessPreview(b.readiness_preview)?.preview_status || "").trim().toLowerCase() === "blocked") ? 1 : 0;
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
  }, [items, query, registryKind, registryPolicy, registrySource]);

  const registryTotalPages = useMemo(
    () => Math.max(1, Math.ceil(Math.max(1, filteredRegistry.length) / Math.max(1, registryPageSize))),
    [filteredRegistry.length, registryPageSize]
  );

  useEffect(() => {
    setRegistryPage(1);
  }, [query, registryKind, registryPolicy, registrySource, registryPageSize]);

  useEffect(() => {
    setRegistryPage((prev) => (prev <= registryTotalPages ? prev : registryTotalPages));
  }, [registryTotalPages]);

  const pagedRegistry = useMemo(() => {
    const safePage = Math.max(1, Math.min(registryPage, registryTotalPages));
    const start = (safePage - 1) * registryPageSize;
    return filteredRegistry.slice(start, start + registryPageSize);
  }, [filteredRegistry, registryPage, registryPageSize, registryTotalPages]);

  const registryRange = useMemo(() => {
    if (!filteredRegistry.length) return { from: 0, to: 0 };
    const safePage = Math.max(1, Math.min(registryPage, registryTotalPages));
    const from = (safePage - 1) * registryPageSize + 1;
    const to = from + pagedRegistry.length - 1;
    return { from, to };
  }, [filteredRegistry.length, registryPage, registryPageSize, registryTotalPages, pagedRegistry.length]);

  const toggleSource = async (sourceId: string, nextEnabled: boolean) => {
    const sid = String(sourceId || "").trim();
    if (!sid) return;
    setBusyKey(`source:${sid}`);
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
      if (idx >= 0) current[idx] = { ...current[idx], enabled: nextEnabled };
      else current.push({ source_id: sid, enabled: nextEnabled, rationale: "" });
      const patchSources = current.map((row) => ({
        source_id: String(row.source_id || "").trim(),
        enabled: Boolean(row.enabled),
        rationale: String(row.rationale || ""),
      }));
      const resp = await api.updateCapabilityAllowlist({ patch: { sources: patchSources } });
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilities.failedSourceToggle"));
        return;
      }
      await load();
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("capabilities.failedSourceToggle"));
    } finally {
      setBusyKey("");
    }
  };

  const updateExternalCapabilitySafetyMode = async (nextMode: ExternalCapabilitySafetyMode) => {
    if (nextMode === externalSafetyMode) return;
    setBusyKey("policy");
    setErr("");
    try {
      const nextLevel = nextMode === "conservative" ? "indexed" : "mounted";
      const sourceLevelPatch = Object.fromEntries(EXTERNAL_SOURCE_IDS.map((sourceId) => [sourceId, nextLevel]));
      const resp = await api.updateCapabilityAllowlist({
        patch: {
          defaults: {
            source_level: sourceLevelPatch,
          },
        },
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilities.failedSafetyMode"));
        return;
      }
      setExternalSafetyMode(normalizeExternalCapabilitySafetyMode(resp.result?.external_capability_safety_mode));
      await load();
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("capabilities.failedSafetyMode"));
    } finally {
      setBusyKey("");
    }
  };

  const toggleBlock = async (row: CapabilityOverviewItem | CapabilityBlockEntry, nextBlocked: boolean) => {
    const capabilityId = String(row.capability_id || "").trim();
    if (!capabilityId) return;
    let reason = "";
    if (nextBlocked) {
      reason = String(window.prompt(t("capabilities.blockReasonPrompt"), (row as CapabilityOverviewItem).blocked_reason || (row as CapabilityBlockEntry).reason || "") || "").trim();
    }
    setBusyKey(`block:${capabilityId}`);
    setErr("");
    try {
      const resp = await api.blockCapabilityGlobal(capabilityId, nextBlocked, reason);
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilities.failedBlock"));
        return;
      }
      await load();
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("capabilities.failedBlock"));
    } finally {
      setBusyKey("");
    }
  };

  return (
    <div className="space-y-4">
      <div className={cardClass(isDark)}>
        <div className="flex items-start justify-between gap-3">
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
        <div className={`text-xs mt-3 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("capabilities.pageGuide")}</div>
        {err ? (
          <div className={`mt-3 text-xs ${isDark ? "text-rose-300" : "text-rose-700"}`} role="alert">{err}</div>
        ) : null}
      </div>

      <div className={cardClass(isDark)}>
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("capabilities.safetyModeTitle")}</div>
            <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("capabilities.safetyModeHint")}</div>
          </div>
          <div className={`text-[11px] ${isDark ? "text-slate-400" : "text-gray-600"}`}>
            {t("capabilities.safetyModeCurrent", { mode: t(`capabilities.safetyMode.${externalSafetyMode}.label`) })}
          </div>
        </div>
        <div className="mt-3 grid gap-2 md:grid-cols-2">
          {(["normal", "conservative"] as ExternalCapabilitySafetyMode[]).map((mode) => {
            const selected = externalSafetyMode === mode;
            return (
              <button
                key={mode}
                type="button"
                className={`rounded-lg border px-3 py-3 text-left ${selected
                  ? isDark
                    ? "border-emerald-700 bg-emerald-950/30"
                    : "border-emerald-300 bg-emerald-50"
                  : isDark
                    ? "border-slate-800 bg-slate-900/40"
                    : "border-gray-200 bg-white"} ${busyKey === "policy" ? "opacity-60 cursor-not-allowed" : ""}`}
                disabled={busyKey === "policy" || selected}
                onClick={() => void updateExternalCapabilitySafetyMode(mode)}
              >
                <div className={`text-sm font-medium ${isDark ? "text-slate-100" : "text-gray-900"}`}>{t(`capabilities.safetyMode.${mode}.label`)}</div>
                <div className={`text-xs mt-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t(`capabilities.safetyMode.${mode}.hint`)}</div>
              </button>
            );
          })}
        </div>
        <div className={`text-[11px] mt-2 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("capabilities.safetyModeCurrentRule", { mode: t(`capabilities.safetyMode.${externalSafetyMode}.label`) })}
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("capabilities.sourcesTitle")}</div>
        <div className={`text-xs mt-1 mb-2 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("capabilities.sourcesHint")}</div>
        <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("capabilities.sourcesSummary", sourceSummary)}
        </div>
        <div className={`text-[11px] mt-0.5 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("capabilities.levelDistribution", levelDistribution)}
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
            <button type="button" onClick={() => setSourceVisibility("all")} className={`px-2.5 py-2 text-xs min-h-[40px] ${sourceVisibility === "all" ? (isDark ? "bg-slate-700 text-slate-100" : "bg-gray-100 text-gray-900") : (isDark ? "bg-slate-900 text-slate-300" : "bg-white text-gray-700")}`}>{t("capabilities.sourcesVisibilityAll")}</button>
            <button type="button" onClick={() => setSourceVisibility("enabled")} className={`px-2.5 py-2 text-xs min-h-[40px] border-l ${isDark ? "border-slate-700" : "border-gray-200"} ${sourceVisibility === "enabled" ? (isDark ? "bg-slate-700 text-slate-100" : "bg-gray-100 text-gray-900") : (isDark ? "bg-slate-900 text-slate-300" : "bg-white text-gray-700")}`}>{t("capabilities.sourcesVisibilityEnabled")}</button>
            <button type="button" onClick={() => setSourceVisibility("disabled")} className={`px-2.5 py-2 text-xs min-h-[40px] border-l ${isDark ? "border-slate-700" : "border-gray-200"} ${sourceVisibility === "disabled" ? (isDark ? "bg-slate-700 text-slate-100" : "bg-gray-100 text-gray-900") : (isDark ? "bg-slate-900 text-slate-300" : "bg-white text-gray-700")}`}>{t("capabilities.sourcesVisibilityDisabled")}</button>
          </div>
        </div>
        <div className="mt-3 space-y-2">
          {sourceRowsVisible.map((row) => {
            const sid = String(row.source_id || "");
            const enabled = Boolean(row.enabled);
            const rationale = String(sourceRationaleMap.get(sid) || row.rationale || "");
            const syncState = String(row.sync_state || "never");
            const count = Number(row.record_count || 0);
            return (
              <div key={sid} className={`rounded-lg border px-3 py-2 ${isDark ? "border-slate-800 bg-slate-900/40" : "border-gray-200 bg-white"}`}>
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className={`text-sm font-medium truncate ${isDark ? "text-slate-100" : "text-gray-900"}`}>{sid}</div>
                    <div className="mt-1 flex flex-wrap gap-1">
                      <span className={`px-1.5 py-0.5 rounded text-[10px] ${enabled ? (isDark ? "bg-emerald-900/40 text-emerald-300" : "bg-emerald-50 text-emerald-700") : (isDark ? "bg-slate-800 text-slate-300" : "bg-gray-100 text-gray-700")}`}>{enabled ? t("capabilities.sourceEnabled") : t("capabilities.sourceDisabled")}</span>
                      <span className={`px-1.5 py-0.5 rounded text-[10px] ${isDark ? "bg-slate-800 text-slate-300" : "bg-gray-100 text-gray-700"}`}>{t("capabilities.sourceMeta", { level: String(row.source_level || "indexed"), sync: syncState, count })}</span>
                    </div>
                    {rationale ? <div className={`text-[11px] mt-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("capabilities.sourceRationale", { text: rationale })}</div> : null}
                  </div>
                  <button
                    type="button"
                    className={`px-2.5 py-1.5 rounded text-xs min-h-[32px] ${enabled ? (isDark ? "bg-rose-900/40 text-rose-300" : "bg-rose-50 text-rose-700 border border-rose-200") : (isDark ? "bg-emerald-900/40 text-emerald-300" : "bg-emerald-50 text-emerald-700 border border-emerald-200")} ${busyKey === `source:${sid}` ? "opacity-60 cursor-not-allowed" : ""}`}
                    disabled={busyKey === `source:${sid}`}
                    onClick={() => void toggleSource(sid, !enabled)}
                  >
                    {enabled ? t("capabilities.sourceDisableAction") : t("capabilities.sourceEnableAction")}
                  </button>
                </div>
              </div>
            );
          })}
          {filteredSources.length > SOURCE_PREVIEW_LIMIT ? (
            <button type="button" className={`text-xs ${isDark ? "text-emerald-300" : "text-emerald-700"}`} onClick={() => setShowAllSources((v) => !v)}>
              {showAllSources ? t("capabilities.showLessSources") : t("capabilities.showMoreSources", { count: filteredSources.length - SOURCE_PREVIEW_LIMIT })}
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
          <select value={registryKind} onChange={(e) => setRegistryKind(e.target.value as RegistryKindFilter)} className={`rounded-lg border px-2 py-2 text-xs min-h-[40px] ${isDark ? "bg-slate-900 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"}`}>
            <option value="all">{t("capabilities.filterKindAll")}</option>
            <option value="pack">{t("capabilities.filterKindPack")}</option>
            <option value="mcp">{t("capabilities.filterKindMcp")}</option>
            <option value="skill">{t("capabilities.filterKindSkill")}</option>
          </select>
          <select value={registryPolicy} onChange={(e) => setRegistryPolicy(e.target.value as RegistryPolicyFilter)} className={`rounded-lg border px-2 py-2 text-xs min-h-[40px] ${isDark ? "bg-slate-900 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"}`}>
            <option value="all">{t("capabilities.filterPolicyAll")}</option>
            <option value="actionable">{t("capabilities.filterPolicyActionable")}</option>
            <option value="blocked">{t("capabilities.filterPolicyBlocked")}</option>
            <option value="indexed">{t("capabilities.filterPolicyIndexed")}</option>
          </select>
          <select value={registrySource} onChange={(e) => setRegistrySource(e.target.value)} className={`rounded-lg border px-2 py-2 text-xs min-h-[40px] ${isDark ? "bg-slate-900 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"}`}>
            <option value="all">{t("capabilities.filterSourceAll")}</option>
            {registrySourceOptions.map((sid) => (<option key={sid} value={sid}>{sid}</option>))}
          </select>
          <div className="grid grid-cols-[auto_minmax(0,1fr)] gap-2 items-center">
            <label className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("capabilities.pageSize")}</label>
            <select value={registryPageSize} onChange={(e) => setRegistryPageSize(Number(e.target.value) || 40)} className={`rounded-lg border px-2 py-2 text-xs min-h-[40px] ${isDark ? "bg-slate-900 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"}`}>
              {REGISTRY_PAGE_SIZE_OPTIONS.map((size) => (<option key={size} value={size}>{size}</option>))}
            </select>
          </div>
        </div>
        <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("capabilities.resultsSummary", { count: filteredRegistry.length })} · {t("capabilities.showingRange", { from: registryRange.from, to: registryRange.to })}
        </div>
        <div className="mt-2 max-h-[420px] overflow-auto space-y-2">
          {pagedRegistry.map((row) => {
            const capId = String(row.capability_id || "");
            const blockedNow = Boolean(row.blocked_global);
            const readinessPreview = normalizeReadinessPreview(row.readiness_preview);
            return (
              <div key={capId} className={`rounded-lg border px-3 py-2 ${isDark ? "border-slate-800 bg-slate-900/40" : "border-gray-200 bg-white"}`}>
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
                      {row.recent_success?.success_count ? <span className={`px-1.5 py-0.5 rounded text-[10px] ${isDark ? "bg-emerald-900/40 text-emerald-300" : "bg-emerald-50 text-emerald-700"}`}>{t("capabilities.recentCount", { count: Number(row.recent_success?.success_count || 0) })}</span> : null}
                      {blockedNow ? <span className={`px-1.5 py-0.5 rounded text-[10px] ${isDark ? "bg-rose-900/40 text-rose-300" : "bg-rose-50 text-rose-700"}`}>{t("capabilities.blocked")}</span> : null}
                    </div>
                    {renderReadinessPreview(readinessPreview)}
                  </div>
                  <div className="flex items-center gap-1.5 shrink-0">
                    <button
                      type="button"
                      className={`px-2.5 py-1.5 rounded text-xs min-h-[32px] ${blockedNow ? (isDark ? "bg-emerald-900/40 text-emerald-300" : "bg-emerald-50 text-emerald-700 border border-emerald-200") : (isDark ? "bg-rose-900/40 text-rose-300" : "bg-rose-50 text-rose-700 border border-rose-200")} ${busyKey === `block:${capId}` ? "opacity-60 cursor-not-allowed" : ""}`}
                      disabled={busyKey === `block:${capId}`}
                      onClick={() => void toggleBlock(row, !blockedNow)}
                    >
                      {blockedNow ? t("capabilities.unblock") : t("capabilities.block")}
                    </button>
                  </div>
                </div>
              </div>
            );
          })}
          {pagedRegistry.length === 0 ? <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("capabilities.noLibraryMatches")}</div> : null}
        </div>
        <div className="mt-2 flex items-center justify-between gap-2">
          <button type="button" className={`px-3 py-1.5 rounded text-xs min-h-[34px] ${isDark ? "bg-slate-800 text-slate-200 disabled:opacity-50" : "bg-gray-100 text-gray-700 disabled:opacity-50"}`} disabled={registryPage <= 1} onClick={() => setRegistryPage((p) => Math.max(1, p - 1))}>{t("capabilities.pagePrev")}</button>
          <div className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("capabilities.pageLabel", { page: registryPage, total: registryTotalPages })}</div>
          <button type="button" className={`px-3 py-1.5 rounded text-xs min-h-[34px] ${isDark ? "bg-slate-800 text-slate-200 disabled:opacity-50" : "bg-gray-100 text-gray-700 disabled:opacity-50"}`} disabled={registryPage >= registryTotalPages} onClick={() => setRegistryPage((p) => Math.min(registryTotalPages, p + 1))}>{t("capabilities.pageNext")}</button>
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("capabilities.blockedListTitle")}</div>
        <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("capabilities.blockedListHint")}</div>
        <div className="mt-2 space-y-2">
          {blocked.length === 0 ? (
            <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("capabilities.noBlocked")}</div>
          ) : (
            blocked.map((row) => {
              const capId = String(row.capability_id || "");
              return (
                <div key={capId} className={`rounded-lg border px-3 py-2 ${isDark ? "border-slate-800 bg-slate-900/40" : "border-gray-200 bg-white"}`}>
                  <div className="flex items-center justify-between gap-2">
                    <code className="text-xs">{capId}</code>
                    <button
                      type="button"
                      className={`px-2.5 py-1 rounded text-xs min-h-[30px] ${isDark ? "bg-emerald-900/40 text-emerald-300" : "bg-emerald-50 text-emerald-700 border border-emerald-200"}`}
                      disabled={busyKey === `block:${capId}`}
                      onClick={() => void toggleBlock(row, false)}
                    >
                      {t("capabilities.unblock")}
                    </button>
                  </div>
                  {String(row.reason || "").trim() ? <div className={`text-[11px] mt-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{String(row.reason || "")}</div> : null}
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}
