import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import { BodyPortal } from "../../ui/BodyPortal";
import { SelectCombobox } from "../../SelectCombobox";
import {
  Actor,
  CapabilityImportRecord,
  CapabilityBlockEntry,
  CapabilityOverviewItem,
  CapabilityReadinessPreview,
  CapabilitySourceState,
  CapabilityUsageActorEntry,
  CapabilityUsageSummary,
  GroupMeta,
} from "../../../types";
import { useModalA11y } from "../../../hooks/useModalA11y";
import {
  inputClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";

interface CapabilitiesTabProps {
  isDark: boolean;
  isActive: boolean;
  groupId?: string;
  surface?: "global" | "selfEvolving";
}

type SourceVisibility = "all" | "enabled" | "disabled";
type RegistryKindFilter = "all" | "pack" | "mcp" | "skill";
type RegistryPolicyFilter = "all" | "actionable" | "blocked" | "indexed";
type ExternalCapabilitySafetyMode = "normal" | "conservative";
type ManageQualificationStatus = "qualified" | "blocked";

const SOURCE_PREVIEW_LIMIT = 8;
const REGISTRY_PAGE_SIZE_OPTIONS = [20, 40, 80];
const CAPABILITY_OVERVIEW_INITIAL_LIMIT = 40;
const CAPABILITY_OVERVIEW_QUERY_DEBOUNCE_MS = 250;
const SELF_PROPOSED_SOURCE_ID = "agent_self_proposed";
const SELF_PROPOSED_CAPSULE_TEXT_MAX = 2400;
const SOURCE_PRIORITY: Record<string, number> = {
  cccc_builtin: 0,
  mcp_registry_official: 1,
  anthropic_skills: 2,
  github_skills_curated: 3,
  agent_self_proposed: 4,
  openclaw_skills_remote: 5,
  clawskills_remote: 6,
  clawhub_remote: 7,
  skillsmp_remote: 8,
  manual_import: 9,
};
// Do not include agent_self_proposed here: source-level mounting would make proposed MCP toolpacks actionable.
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

function firstRecommendationLine(value?: string[]) {
  return Array.isArray(value) ? String(value[0] || "").trim() : "";
}

function selfProposedFallbackCapsule(row: CapabilityOverviewItem) {
  const name = String(row.name || row.capability_id || "Self-Proposed Skill").trim();
  const description = String(row.description_short || "Maintain a reusable self-proposed procedure.").trim();
  return [
    `Skill: ${name}`,
    "When to use:",
    `- ${description}`,
    "Avoid when:",
    "- The lesson is one-off, unverified, or belongs in memory/task notes instead of a skill.",
    "Procedure:",
    "1. Search existing self-proposed skills first.",
    "2. Reuse the same capability_id when updating this workflow.",
    "Pitfalls:",
    "- Do not create a near-duplicate or silently delete the candidate.",
    "Verification:",
    "- Re-import the record and verify it appears under agent_self_proposed.",
  ].join("\n");
}

function normalizeCapabilityIdList(raw: unknown) {
  const out: string[] = [];
  if (Array.isArray(raw)) {
    for (const item of raw) {
      const value = String(item || "").trim();
      if (value && !out.includes(value)) out.push(value);
    }
  }
  return out;
}

function capabilitySlugTail(row: CapabilityOverviewItem) {
  const capId = String(row.capability_id || "").trim().toLowerCase();
  return capId.split(":").filter(Boolean).pop() || capId;
}

function capabilityUsageActorLabel(row: CapabilityUsageActorEntry) {
  return String(row.label || row.actor_title || row.actor_id || "").trim() || "user";
}

function capabilityEnableResultSucceeded(result: unknown) {
  if (!result || typeof result !== "object") return false;
  const row = result as Record<string, unknown>;
  const state = String(row.state || "").trim().toLowerCase();
  return row.enabled === true && !["blocked", "denied", "failed"].includes(state);
}

function capabilityEnableResultReason(result: unknown) {
  if (!result || typeof result !== "object") return "";
  const row = result as Record<string, unknown>;
  return String(row.reason || row.state || row.policy_level || "").trim();
}

function deriveManagedAssignedActorIds(
  actors: Actor[],
  capabilityId: string,
  usage: CapabilityUsageSummary | null,
) {
  const capId = String(capabilityId || "").trim();
  if (!capId) return [];
  const assigned = new Set<string>();
  const actorIds = actors.map((actor) => String(actor.id || "").trim()).filter(Boolean);
  for (const actor of actors) {
    const actorId = String(actor.id || "").trim();
    if (actorId && normalizeCapabilityIdList(actor.capability_autoload).includes(capId)) {
      assigned.add(actorId);
    }
  }
  if (usage?.group_enabled) {
    for (const actorId of actorIds) assigned.add(actorId);
  }
  for (const row of usage?.actor_enabled || []) {
    const actorId = String(row.actor_id || "").trim();
    if (actorId) assigned.add(actorId);
  }
  for (const row of usage?.actor_autoload || []) {
    const actorId = String(row.actor_id || "").trim();
    if (actorId) assigned.add(actorId);
  }
  return actorIds.filter((actorId) => assigned.has(actorId));
}

function formatCapabilityProvenanceTimestamp(value: unknown) {
  const raw = String(value || "").trim();
  if (!raw) return "";
  const ms = Date.parse(raw);
  if (!Number.isFinite(ms)) return raw;
  return new Date(ms).toLocaleString();
}

export function CapabilitiesTab({ isDark: _isDark, isActive, groupId = "", surface = "global" }: CapabilitiesTabProps) {
  const { t } = useTranslation("settings");
  const selfEvolvingSurface = surface === "selfEvolving";
  const [loading, setLoading] = useState(false);
  const [busyKey, setBusyKey] = useState("");
  const [err, setErr] = useState("");
  const [manageErr, setManageErr] = useState("");
  const [manageNotice, setManageNotice] = useState("");
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [sourceQuery, setSourceQuery] = useState("");
  const [sourceVisibility, setSourceVisibility] = useState<SourceVisibility>("all");
  const [showAllSources, setShowAllSources] = useState(false);
  const [registryKind, setRegistryKind] = useState<RegistryKindFilter>("all");
  const [registryPolicy, setRegistryPolicy] = useState<RegistryPolicyFilter>("all");
  const [registrySource, setRegistrySource] = useState("all");
  const [registryPageSize, setRegistryPageSize] = useState(40);
  const [items, setItems] = useState<CapabilityOverviewItem[]>([]);
  const [registryTotalCount, setRegistryTotalCount] = useState(0);
  const [registryHasMore, setRegistryHasMore] = useState(false);
  const [groups, setGroups] = useState<GroupMeta[]>([]);
  const [sources, setSources] = useState<Record<string, CapabilitySourceState>>({});
  const [blocked, setBlocked] = useState<CapabilityBlockEntry[]>([]);
  const [allowlistSources, setAllowlistSources] = useState<Array<{ source_id: string; enabled: boolean; rationale?: string }>>([]);
  const [externalSafetyMode, setExternalSafetyMode] = useState<ExternalCapabilitySafetyMode>("normal");
  const [manageCapabilityId, setManageCapabilityId] = useState("");
  const [manageName, setManageName] = useState("");
  const [manageDescription, setManageDescription] = useState("");
  const [manageCapsuleText, setManageCapsuleText] = useState("");
  const [manageQualificationStatus, setManageQualificationStatus] = useState<ManageQualificationStatus>("qualified");
  const [manageQualificationReason, setManageQualificationReason] = useState("");
  const [manageActors, setManageActors] = useState<Actor[]>([]);
  const [manageAssignedActorIds, setManageAssignedActorIds] = useState<string[]>([]);
  const [manageUsage, setManageUsage] = useState<CapabilityUsageSummary | null>(null);
  const [manageUsageLoading, setManageUsageLoading] = useState(false);
  const overviewRequestSeqRef = useRef(0);
  const overviewItemCountRef = useRef(0);
  const registryListRef = useRef<HTMLDivElement | null>(null);
  const registryLoadMoreRef = useRef<HTMLDivElement | null>(null);

  const closeSelfProposedManager = useCallback(() => {
    setManageCapabilityId("");
    setManageAssignedActorIds([]);
    setManageUsage(null);
    setManageUsageLoading(false);
    setManageErr("");
    setManageNotice("");
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebouncedQuery(String(query || "").trim());
    }, CAPABILITY_OVERVIEW_QUERY_DEBOUNCE_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [query]);

  const load = useCallback(async (opts?: { append?: boolean }) => {
    if (!isActive) return;
    const append = opts?.append === true;
    const requestSeq = overviewRequestSeqRef.current + 1;
    overviewRequestSeqRef.current = requestSeq;
    setLoading(true);
    setErr("");
    try {
      const overviewQuery = String(debouncedQuery || "").trim();
      const nextOffset = append ? overviewItemCountRef.current : 0;
      const [overviewResp, allowlistResp, groupsResp] = await Promise.all([
        api.fetchCapabilityOverview({
          includeIndexed: true,
          limit: registryPageSize || CAPABILITY_OVERVIEW_INITIAL_LIMIT,
          offset: nextOffset,
          query: overviewQuery || undefined,
          kind: registryKind,
          policy: registryPolicy,
          sourceId: registrySource,
        }),
        append || selfEvolvingSurface ? Promise.resolve(null) : api.fetchCapabilityAllowlist(),
        append || selfEvolvingSurface ? Promise.resolve(null) : api.fetchGroups(),
      ]);
      if (overviewRequestSeqRef.current != requestSeq) return;
      if (!overviewResp.ok) {
        setErr(overviewResp.error?.message || t("capabilities.failedLoad"));
        setItems([]);
        overviewItemCountRef.current = 0;
        setRegistryTotalCount(0);
        setRegistryHasMore(false);
        setGroups([]);
        setSources({});
        setBlocked([]);
      } else {
        const nextItems = Array.isArray(overviewResp.result?.items) ? overviewResp.result.items : [];
        setItems((current) => {
          const merged = append ? [...current, ...nextItems] : nextItems;
          overviewItemCountRef.current = merged.length;
          return merged;
        });
        setRegistryTotalCount(Math.max(0, Number(overviewResp.result?.total_count || 0)));
        setRegistryHasMore(Boolean(overviewResp.result?.has_more));
        if (!append) {
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
      }
      if (!append && groupsResp?.ok) {
        setGroups(Array.isArray(groupsResp.result?.groups) ? groupsResp.result.groups : []);
      } else if (!append && !selfEvolvingSurface) {
        setGroups([]);
      }
      if (!append && allowlistResp?.ok) {
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
      if (overviewRequestSeqRef.current != requestSeq) return;
      setErr(e instanceof Error ? e.message : t("capabilities.failedLoad"));
      if (!append) {
        setItems([]);
        overviewItemCountRef.current = 0;
        setRegistryTotalCount(0);
        setRegistryHasMore(false);
        setGroups([]);
        setSources({});
        setBlocked([]);
      }
    } finally {
      if (overviewRequestSeqRef.current === requestSeq) {
        setLoading(false);
      }
    }
  }, [debouncedQuery, isActive, registryKind, registryPageSize, registryPolicy, registrySource, selfEvolvingSurface, t]);

  useEffect(() => {
    if (!isActive) return;
    void load();
  }, [isActive, load]);

  useEffect(() => {
    const root = registryListRef.current;
    const target = registryLoadMoreRef.current;
    if (!root || !target || typeof IntersectionObserver === "undefined") return;
    if (!isActive || selfEvolvingSurface || !registryHasMore || loading) return;

    const observer = new IntersectionObserver(
      (entries) => {
        const entry = entries[0];
        if (!entry?.isIntersecting) return;
        void load({ append: true });
      },
      {
        root,
        rootMargin: "0px 0px 160px 0px",
        threshold: 0,
      },
    );

    observer.observe(target);
    return () => observer.disconnect();
  }, [isActive, load, loading, registryHasMore, selfEvolvingSurface]);

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

  const selfProposedCandidates = useMemo(() => {
    const gid = String(groupId || "").trim();
    return items.filter((row) => (
      String(row.source_id || "").trim() === SELF_PROPOSED_SOURCE_ID
      && String(row.kind || "").trim().toLowerCase() === "skill"
      && (!selfEvolvingSurface || !gid || String(row.origin_group_id || "").trim() === gid)
    ));
  }, [groupId, items, selfEvolvingSurface]);

  const selfProposedGroupSections = useMemo(() => {
    const groupById = new Map<string, GroupMeta>();
    for (const group of groups) {
      const gid = String(group.group_id || "").trim();
      if (gid) groupById.set(gid, group);
    }
    const sections = new Map<string, { key: string; groupId: string; label: string; hint: string; rows: CapabilityOverviewItem[] }>();
    for (const row of selfProposedCandidates) {
      const originGroupId = String(row.origin_group_id || "").trim();
      const key = originGroupId || "__ungrouped__";
      const group = originGroupId ? groupById.get(originGroupId) : null;
      const title = group ? String(group.title || group.topic || "").trim() : "";
      const label = originGroupId
        ? (title || originGroupId)
        : t("capabilities.selfProposedUngroupedTitle");
      const hint = originGroupId
        ? originGroupId
        : t("capabilities.selfProposedUngroupedHint");
      const existing = sections.get(key);
      if (existing) existing.rows.push(row);
      else sections.set(key, { key, groupId: originGroupId, label, hint, rows: [row] });
    }
    return Array.from(sections.values()).sort((a, b) => {
      if (!a.groupId && b.groupId) return 1;
      if (a.groupId && !b.groupId) return -1;
      return a.label.localeCompare(b.label);
    });
  }, [groups, selfProposedCandidates, t]);

  const managingCandidate = useMemo(() => {
    if (!manageCapabilityId) return null;
    return selfProposedCandidates.find((row) => String(row.capability_id || "").trim() === manageCapabilityId) || null;
  }, [manageCapabilityId, selfProposedCandidates]);

  const manageDuplicateCandidates = useMemo(() => {
    if (!managingCandidate) return [];
    const targetName = String(managingCandidate.name || "").trim().toLowerCase();
    const targetSlug = capabilitySlugTail(managingCandidate);
    return selfProposedCandidates
      .filter((row) => String(row.capability_id || "").trim() !== manageCapabilityId)
      .filter((row) => {
        const name = String(row.name || "").trim().toLowerCase();
        const slug = capabilitySlugTail(row);
        return Boolean((targetName && name === targetName) || (targetSlug && slug === targetSlug));
      })
      .slice(0, 3);
  }, [manageCapabilityId, managingCandidate, selfProposedCandidates]);

  const { modalRef: manageDialogRef } = useModalA11y(Boolean(managingCandidate), closeSelfProposedManager);

  const manageUsageTtlLabel = useCallback((seconds?: number) => {
    const safeSeconds = Number.isFinite(Number(seconds)) ? Math.max(0, Math.trunc(Number(seconds))) : 0;
    if (safeSeconds < 60) return t("capabilities.manageUsageTtlSeconds");
    if (safeSeconds < 3600) return t("capabilities.manageUsageTtlMinutes", { count: Math.ceil(safeSeconds / 60) });
    return t("capabilities.manageUsageTtlHours", { count: Math.ceil(safeSeconds / 3600) });
  }, [t]);

  const manageAssignedActorIdSet = useMemo(() => new Set(manageAssignedActorIds), [manageAssignedActorIds]);

  const manageProfileActorIdSet = useMemo(() => {
    return new Set((manageUsage?.profile_autoload || []).map((row) => String(row.actor_id || "").trim()).filter(Boolean));
  }, [manageUsage]);

  const manageSessionActorIdSet = useMemo(() => {
    return new Set((manageUsage?.session_enabled || []).map((row) => String(row.actor_id || "").trim()).filter(Boolean));
  }, [manageUsage]);

  const manageActorScopeIdSet = useMemo(() => {
    return new Set((manageUsage?.actor_enabled || []).map((row) => String(row.actor_id || "").trim()).filter(Boolean));
  }, [manageUsage]);

  const manageProvenanceRows = useMemo(() => {
    if (!managingCandidate) return [];
    const recordId = String(managingCandidate.source_record_id || manageCapabilityId || "").trim();
    const recordVersion = String(managingCandidate.source_record_version || "").trim();
    const sourceTier = String(managingCandidate.source_tier || "").trim();
    const trustTier = String(managingCandidate.trust_tier || "").trim();
    const originGroupId = String(managingCandidate.origin_group_id || "").trim();
    const updatedAt = formatCapabilityProvenanceTimestamp(managingCandidate.updated_at_source);
    const importedAt = formatCapabilityProvenanceTimestamp(managingCandidate.last_synced_at);
    const status = manageQualificationStatus === "blocked"
      ? t("capabilities.manageStatusBlocked")
      : t("capabilities.manageStatusAvailable");
    const rows = [
      {
        label: t("capabilities.manageProvenanceSource"),
        value: String(managingCandidate.source_id || SELF_PROPOSED_SOURCE_ID).trim() || SELF_PROPOSED_SOURCE_ID,
      },
      {
        label: t("capabilities.manageProvenanceRecord"),
        value: recordId || t("capabilities.manageProvenanceNotRecorded"),
      },
    ];
    if (originGroupId) {
      rows.push({
        label: t("capabilities.manageProvenanceOriginGroup"),
        value: originGroupId,
      });
    }
    if (recordVersion) {
      rows.push({
        label: t("capabilities.manageProvenanceVersion"),
        value: recordVersion,
      });
    }
    rows.push(
      {
        label: t("capabilities.manageProvenanceUpdated"),
        value: updatedAt || t("capabilities.manageProvenanceNotRecorded"),
      },
      {
        label: t("capabilities.manageProvenanceImported"),
        value: importedAt || t("capabilities.manageProvenanceNotRecorded"),
      },
      {
        label: t("capabilities.manageProvenanceTrust"),
        value: [trustTier, sourceTier].filter(Boolean).join(" / ") || t("capabilities.manageProvenanceNotRecorded"),
      },
      {
        label: t("capabilities.manageProvenanceAvailability"),
        value: status,
      },
    );
    const blockReason = String(manageQualificationReason || "").trim();
    if (manageQualificationStatus === "blocked" && blockReason) {
      rows.push({
        label: t("capabilities.manageProvenanceBlockReason"),
        value: blockReason,
      });
    }
    return rows;
  }, [manageCapabilityId, manageQualificationReason, manageQualificationStatus, managingCandidate, t]);

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
    out.add(SELF_PROPOSED_SOURCE_ID);
    for (const row of sourceRows) {
      const sid = String(row.source_id || "").trim();
      if (sid) out.add(sid);
    }
    return Array.from(out).sort((a, b) => {
      const aPriority = SOURCE_PRIORITY[a] ?? 99;
      const bPriority = SOURCE_PRIORITY[b] ?? 99;
      if (aPriority !== bPriority) return aPriority - bPriority;
      return a.localeCompare(b);
    });
  }, [sourceRows]);

  const readinessBadgeClass = (status: string) => {
    if (status === "blocked") {
      return "bg-rose-500/15 text-rose-600 dark:text-rose-400";
    }
    if (status === "enableable") {
      return "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400";
    }
    return "bg-amber-500/15 text-amber-600 dark:text-amber-400";
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
      <div className="mt-2 rounded-md border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-2 py-1.5">
        <div className="flex flex-wrap items-center gap-1.5">
          <span className={`px-1.5 py-0.5 rounded text-[10px] ${readinessBadgeClass(status)}`}>{statusLabel}</span>
          {reasonLabel ? (
            <span className="text-[11px] text-[var(--color-text-secondary)]">{reasonLabel}</span>
          ) : null}
        </div>
        {nextLabel ? (
          <div className="text-[11px] mt-1 text-[var(--color-text-tertiary)]">
            {t("capabilities.readiness.nextLabel")}: {nextLabel}
          </div>
        ) : null}
        {missingEnv.length ? (
          <div className="text-[11px] mt-1 text-[var(--color-text-tertiary)]">
            {t("capabilities.readiness.missingEnv", { names: missingEnv.join(", ") })}
          </div>
        ) : null}
      </div>
    );
  };

  const registryRange = useMemo(() => {
    if (!items.length) return { from: 0, to: 0 };
    const from = 1;
    const to = items.length;
    return { from, to };
  }, [items.length]);

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
    if (!String(groupId || "").trim()) {
      setErr(t("capabilities.requireGroup"));
      return;
    }
    let reason = "";
    if (nextBlocked) {
      reason = String(window.prompt(t("capabilities.blockReasonPrompt"), (row as CapabilityOverviewItem).blocked_reason || (row as CapabilityBlockEntry).reason || "") || "").trim();
    }
    setBusyKey(`block:${capabilityId}`);
    setErr("");
    try {
      const resp = await api.blockCapabilityGlobal(capabilityId, nextBlocked, reason, String(groupId || "").trim());
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

  const refreshManageAssignmentState = async (capabilityId: string = manageCapabilityId) => {
    const gid = String(groupId || "").trim();
    const capId = String(capabilityId || "").trim();
    if (!gid) {
      setManageActors([]);
      setManageAssignedActorIds([]);
      setManageUsage(null);
      setManageUsageLoading(false);
      return;
    }
    setManageUsageLoading(true);
    try {
      const [actorsResp, usageResp] = await Promise.all([
        api.fetchActors(gid, false, { noCache: true }),
        capId
          ? api.fetchGroupCapabilityState(gid, "user", {
              capabilityId: capId,
              noCache: true,
            })
          : Promise.resolve(null),
      ]);
      const actors = actorsResp.ok && Array.isArray(actorsResp.result?.actors) ? actorsResp.result.actors : [];
      const usage = usageResp && usageResp.ok ? usageResp.result?.capability_usage || null : null;
      setManageActors(actors);
      setManageUsage(usage);
      setManageAssignedActorIds(deriveManagedAssignedActorIds(actors, capId, usage));
      if (!actorsResp.ok) {
        setManageErr(actorsResp.error?.message || t("capabilities.manageActorLoadFailed"));
      } else if (usageResp && !usageResp.ok) {
        setManageErr(usageResp.error?.message || t("capabilities.manageUsageLoadFailed"));
      }
    } catch (e) {
      setManageActors([]);
      setManageAssignedActorIds([]);
      setManageUsage(null);
      setManageErr(e instanceof Error ? e.message : t("capabilities.manageUsageLoadFailed"));
    } finally {
      setManageUsageLoading(false);
    }
  };

  const refreshManageUsage = async (capabilityId: string = manageCapabilityId) => {
    const gid = String(groupId || "").trim();
    const capId = String(capabilityId || "").trim();
    if (!gid || !capId) {
      setManageUsage(null);
      setManageUsageLoading(false);
      return;
    }
    setManageUsageLoading(true);
    try {
      const resp = await api.fetchGroupCapabilityState(gid, "user", {
        capabilityId: capId,
        noCache: true,
      });
      if (!resp.ok) {
        setManageUsage(null);
        setManageErr(resp.error?.message || t("capabilities.manageUsageLoadFailed"));
        return;
      }
      setManageUsage(resp.result?.capability_usage || null);
    } catch (e) {
      setManageUsage(null);
      setManageErr(e instanceof Error ? e.message : t("capabilities.manageUsageLoadFailed"));
    } finally {
      setManageUsageLoading(false);
    }
  };

  const openSelfProposedManager = (row: CapabilityOverviewItem) => {
    const capId = String(row.capability_id || "").trim();
    if (!capId) return;
    setManageCapabilityId(capId);
    setManageName(String(row.name || capId));
    setManageDescription(String(row.description_short || ""));
    setManageCapsuleText(String(row.capsule_text || "").trim() || selfProposedFallbackCapsule(row));
    setManageQualificationStatus(String(row.qualification_status || "").trim().toLowerCase() === "blocked" ? "blocked" : "qualified");
    const reasons = Array.isArray(row.qualification_reasons) ? row.qualification_reasons : [];
    setManageQualificationReason(String(row.blocked_reason || reasons[0] || ""));
    setManageErr("");
    setManageNotice("");
    void refreshManageAssignmentState(capId);
  };

  const saveManagedSelfProposed = async (
    qualificationOverride?: ManageQualificationStatus,
    noticeKey: string = "capabilities.manageSaved",
  ) => {
    const gid = String(groupId || "").trim();
    const capId = String(manageCapabilityId || "").trim();
    const capsuleText = String(manageCapsuleText || "").trim();
    const nextQualification = qualificationOverride || manageQualificationStatus;
    if (!gid) {
      setManageErr(t("capabilities.manageRequiresGroup"));
      return;
    }
    if (!capId || !managingCandidate) {
      setManageErr(t("capabilities.manageMissingCandidate"));
      return;
    }
    if (!capId.startsWith("skill:agent_self_proposed:")) {
      setManageErr(t("capabilities.manageInvalidNamespace"));
      return;
    }
    if (!capsuleText) {
      setManageErr(t("capabilities.manageCapsuleRequired"));
      return;
    }
    const qualificationReasons = nextQualification === "blocked"
      ? [String(manageQualificationReason || "manual_review_required").trim() || "manual_review_required"]
      : [];
    const record: CapabilityImportRecord = {
      capability_id: capId,
      kind: "skill",
      source_id: SELF_PROPOSED_SOURCE_ID,
      name: String(manageName || managingCandidate.name || capId).trim(),
      description_short: String(manageDescription || managingCandidate.description_short || "").trim(),
      source_uri: String(managingCandidate.source_uri || ""),
      source_record_id: String(managingCandidate.source_record_id || capId),
      source_record_version: String(managingCandidate.source_record_version || ""),
      origin_group_id: String(managingCandidate.origin_group_id || gid),
      updated_at_source: String(managingCandidate.updated_at_source || ""),
      trust_tier: String(managingCandidate.trust_tier || "tier2"),
      source_tier: String(managingCandidate.source_tier || "tier2"),
      tags: Array.isArray(managingCandidate.tags) ? managingCandidate.tags : [],
      qualification_status: nextQualification,
      qualification_reasons: qualificationReasons,
      capsule_text: capsuleText,
    };
    setBusyKey(`manage:${capId}`);
    setManageErr("");
    setManageNotice("");
    try {
      const resp = await api.importCapability(gid, record, {
        dryRun: false,
        enableAfterImport: false,
        actorId: "user",
        reason: "web_self_proposed_manage",
      });
      if (!resp.ok) {
        setManageErr(resp.error?.message || t("capabilities.manageSaveFailed"));
        return;
      }
      const savedRecord = resp.result?.record && typeof resp.result.record === "object"
        ? (resp.result.record as Record<string, unknown>)
        : {};
      const savedQualification = String(savedRecord.qualification_status || "").trim().toLowerCase();
      if (savedQualification === "blocked" || savedQualification === "qualified") {
        setManageQualificationStatus(savedQualification);
      }
      const savedReasons = Array.isArray(savedRecord.qualification_reasons)
        ? savedRecord.qualification_reasons.map((item) => String(item || "").trim()).filter(Boolean)
        : [];
      if (savedReasons[0]) setManageQualificationReason(savedReasons[0]);
      const savedCapsuleText = String(savedRecord.capsule_text || "").trim();
      if (savedCapsuleText) setManageCapsuleText(savedCapsuleText);
      setManageNotice(t(noticeKey));
      await load();
      await refreshManageUsage(capId);
    } catch (e) {
      setManageErr(e instanceof Error ? e.message : t("capabilities.manageSaveFailed"));
    } finally {
      setBusyKey("");
    }
  };

  const toggleManagedActorAssignment = (actorId: string) => {
    const aid = String(actorId || "").trim();
    if (!aid) return;
    setManageAssignedActorIds((current) => {
      if (current.includes(aid)) return current.filter((item) => item !== aid);
      return [...current, aid];
    });
  };

  const saveManagedActorAssignments = async () => {
    const gid = String(groupId || "").trim();
    const capId = String(manageCapabilityId || "").trim();
    if (!gid) {
      setManageErr(t("capabilities.manageRequiresGroup"));
      return;
    }
    if (!capId) return;
    setBusyKey(`manage-use:${capId}`);
    setManageErr("");
    setManageNotice("");
    try {
      const actorsResp = await api.fetchActors(gid, false, { noCache: true });
      if (!actorsResp.ok) {
        setManageErr(actorsResp.error?.message || t("capabilities.manageActorLoadFailed"));
        return;
      }
      const actors = Array.isArray(actorsResp.result?.actors) ? actorsResp.result.actors : [];
      const desired = new Set(manageAssignedActorIds.map((item) => String(item || "").trim()).filter(Boolean));
      const userSessionResp = await api.enableGroupCapability(gid, capId, {
        enabled: false,
        scope: "session",
        actorId: "user",
        reason: "web_self_proposed_actor_assignment",
      });
      if (!userSessionResp.ok) {
        setManageErr(userSessionResp.error?.message || t("capabilities.manageActorAssignmentsFailed"));
        return;
      }
      for (const actor of actors) {
        const aid = String(actor.id || "").trim();
        if (!aid) continue;
        const currentAutoload = normalizeCapabilityIdList(actor.capability_autoload);
        const hasAutoload = currentAutoload.includes(capId);
        const shouldAutoload = desired.has(aid);
        if (shouldAutoload) {
          const actorResp = await api.enableGroupCapability(gid, capId, {
            enabled: true,
            scope: "actor",
            actorId: aid,
            reason: "web_self_proposed_actor_assignment",
          });
          if (!actorResp.ok) {
            setManageErr(actorResp.error?.message || t("capabilities.manageActorAssignmentsFailed"));
            return;
          }
          if (!capabilityEnableResultSucceeded(actorResp.result)) {
            const reason = capabilityEnableResultReason(actorResp.result);
            setManageErr(
              reason
                ? t("capabilities.manageActorActivationFailedWithReason", { reason })
                : t("capabilities.manageActorActivationFailed")
            );
            return;
          }
          if (!hasAutoload) {
            const resp = await api.updateActor(gid, aid, undefined, undefined, undefined, undefined, {
              capabilityAutoload: [...currentAutoload, capId],
            });
            if (!resp.ok) {
              await api.enableGroupCapability(gid, capId, {
                enabled: false,
                scope: "actor",
                actorId: aid,
                reason: "web_self_proposed_actor_assignment_rollback",
              });
              setManageErr(resp.error?.message || t("capabilities.manageActorAssignmentsFailed"));
              return;
            }
          }
        } else {
          if (hasAutoload) {
            const resp = await api.updateActor(gid, aid, undefined, undefined, undefined, undefined, {
              capabilityAutoload: currentAutoload.filter((item) => item !== capId),
            });
            if (!resp.ok) {
              setManageErr(resp.error?.message || t("capabilities.manageActorAssignmentsFailed"));
              return;
            }
          }
          const actorResp = await api.enableGroupCapability(gid, capId, {
            enabled: false,
            scope: "actor",
            actorId: aid,
            reason: "web_self_proposed_actor_assignment",
          });
          if (!actorResp.ok) {
            setManageErr(actorResp.error?.message || t("capabilities.manageActorAssignmentsFailed"));
            return;
          }
          const sessionResp = await api.enableGroupCapability(gid, capId, {
            enabled: false,
            scope: "session",
            actorId: aid,
            reason: "web_self_proposed_actor_assignment",
          });
          if (!sessionResp.ok) {
            setManageErr(sessionResp.error?.message || t("capabilities.manageActorAssignmentsFailed"));
            return;
          }
        }
      }
      if (manageUsage?.group_enabled) {
        const groupResp = await api.enableGroupCapability(gid, capId, {
          enabled: false,
          scope: "group",
          actorId: "user",
          reason: "web_self_proposed_actor_assignment",
        });
        if (!groupResp.ok) {
          setManageErr(groupResp.error?.message || t("capabilities.manageActorAssignmentsFailed"));
          return;
        }
      }
      setManageNotice(t("capabilities.manageActorAssignmentsSaved"));
      await refreshManageAssignmentState(capId);
      await load();
    } catch (e) {
      setManageErr(e instanceof Error ? e.message : t("capabilities.manageActorAssignmentsFailed"));
    } finally {
      setBusyKey("");
    }
  };

  const uninstallManagedSelfProposed = async () => {
    const gid = String(groupId || "").trim();
    const capId = String(manageCapabilityId || "").trim();
    if (!gid) {
      setManageErr(t("capabilities.manageRequiresGroup"));
      return;
    }
    if (!capId) return;
    if (typeof window !== "undefined" && !window.confirm(t("capabilities.manageRemoveConfirm"))) return;
    setBusyKey(`manage-remove:${capId}`);
    setManageErr("");
    setManageNotice("");
    try {
      const resp = await api.uninstallCapability(gid, capId, {
        actorId: "user",
        reason: "web_self_proposed_uninstall",
      });
      if (!resp.ok) {
        setManageErr(resp.error?.message || t("capabilities.manageRemoveFailed"));
        return;
      }
      closeSelfProposedManager();
      await load();
    } catch (e) {
      setManageErr(e instanceof Error ? e.message : t("capabilities.manageRemoveFailed"));
    } finally {
      setBusyKey("");
    }
  };

  return (
    <div className="space-y-5">
      {!selfEvolvingSurface ? (
        <section className={settingsWorkspaceShellClass(_isDark)}>
          <div className={settingsWorkspaceHeaderClass(_isDark)}>
            <div>
              <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("capabilities.title")}</div>
              <div className="mt-1 text-xs text-[var(--color-text-muted)]">{t("capabilities.subtitle")}</div>
            </div>
            <button
              type="button"
              className={secondaryButtonClass("sm")}
              onClick={() => void load()}
              disabled={loading}
            >
              {loading ? t("common:loading") : t("capabilities.refresh")}
            </button>
          </div>
          <div className={settingsWorkspaceBodyClass}>
            <div className="text-xs text-[var(--color-text-tertiary)]">{t("capabilities.pageGuide")}</div>
            {err ? (
              <div className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-600 dark:text-rose-400" role="alert">{err}</div>
            ) : null}
          </div>
        </section>
      ) : err ? (
        <div className="rounded-xl border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-600 dark:text-rose-400" role="alert">
          {err}
        </div>
      ) : null}

      <section className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
          <div>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">
              {t(selfEvolvingSurface ? "capabilities.selfEvolvingGroupTitle" : "capabilities.selfProposedTitle")}
            </div>
            <div className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t(selfEvolvingSurface ? "capabilities.selfEvolvingGroupHint" : "capabilities.selfProposedHint")}
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              className={secondaryButtonClass("sm")}
              onClick={() => void load()}
              disabled={loading}
            >
              {loading ? t("common:loading") : t("capabilities.refresh")}
            </button>
          </div>
        </div>
        <div className={settingsWorkspaceBodyClass}>
          <div className="grid gap-3 md:grid-cols-2">
            <div className={settingsWorkspacePanelClass(_isDark)}>
              <div className="text-[10px] uppercase tracking-[0.12em] text-[var(--color-text-muted)]">
                {t(selfEvolvingSurface ? "capabilities.selfEvolvingGroupCount" : "capabilities.selfProposedGenerated")}
              </div>
              <div className="mt-1 text-lg font-semibold text-[var(--color-text-primary)]">{selfProposedCandidates.length}</div>
            </div>
            <div className={settingsWorkspacePanelClass(_isDark)}>
              <div className="text-[10px] uppercase tracking-[0.12em] text-[var(--color-text-muted)]">
                {t(selfEvolvingSurface ? "capabilities.selfProposedSource" : "capabilities.selfProposedGroups")}
              </div>
              <div className={`${selfEvolvingSurface ? "text-sm font-mono" : "text-lg font-semibold"} mt-1 text-[var(--color-text-primary)]`}>
                {selfEvolvingSurface ? SELF_PROPOSED_SOURCE_ID : selfProposedGroupSections.length}
              </div>
            </div>
          </div>
        {selfEvolvingSurface ? (
          <div className="space-y-3">
            {selfProposedCandidates.map((row) => {
              const capId = String(row.capability_id || "");
              const isBlocked = String(row.qualification_status || "").trim().toLowerCase() === "blocked";
              return (
                <div key={capId} className={settingsWorkspacePanelClass(_isDark)}>
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="text-xs font-medium text-[var(--color-text-primary)]">{String(row.name || capId)}</span>
                    {isBlocked ? (
                      <span className="rounded bg-rose-500/10 px-1.5 py-0.5 text-[10px] text-rose-600 dark:text-rose-300">
                        {t("capabilities.manageStatusBlocked")}
                      </span>
                    ) : null}
                  </div>
                  <div className="mt-0.5 text-[11px] truncate text-[var(--color-text-tertiary)]">{capId}</div>
                  {String(row.description_short || "").trim() ? (
                    <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">{String(row.description_short || "")}</div>
                  ) : null}
                  <div className="mt-3">
                    <button
                      type="button"
                      className={secondaryButtonClass("sm")}
                      onClick={() => openSelfProposedManager(row)}
                    >
                      {t("capabilities.selfProposedManage")}
                    </button>
                  </div>
                </div>
              );
            })}
            {selfProposedCandidates.length === 0 ? (
              <div className="text-xs text-[var(--color-text-muted)]">
                {t("capabilities.selfEvolvingGroupNoCandidates")}
              </div>
            ) : null}
          </div>
        ) : (
          <div className="space-y-3">
            {selfProposedGroupSections.map((section) => (
              <div key={section.key} className={settingsWorkspacePanelClass(_isDark)}>
                <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                  <div className="min-w-0">
                    <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">{section.label}</div>
                    <div className="mt-0.5 font-mono text-[11px] text-[var(--color-text-tertiary)]">{section.hint}</div>
                  </div>
                  <span className="w-fit rounded-full bg-[var(--glass-tab-bg)] px-2 py-1 text-[10px] font-medium text-[var(--color-text-secondary)]">
                    {t("capabilities.selfProposedGroupSkillCount", { count: section.rows.length })}
                  </span>
                </div>
                <div className="mt-3 space-y-2">
                  {section.rows.map((row) => {
                    const capId = String(row.capability_id || "");
                    const isBlocked = String(row.qualification_status || "").trim().toLowerCase() === "blocked";
                    return (
                      <div key={capId} className={settingsWorkspaceSoftPanelClass(_isDark)}>
                        <div className="flex flex-wrap items-center gap-1.5">
                          <span className="text-xs font-medium text-[var(--color-text-primary)]">{String(row.name || capId)}</span>
                          {isBlocked ? (
                            <span className="rounded bg-rose-500/10 px-1.5 py-0.5 text-[10px] text-rose-600 dark:text-rose-300">
                              {t("capabilities.manageStatusBlocked")}
                            </span>
                          ) : null}
                        </div>
                        <div className="mt-0.5 text-[11px] truncate text-[var(--color-text-tertiary)]">{capId}</div>
                        {String(row.description_short || "").trim() ? (
                          <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">{String(row.description_short || "")}</div>
                        ) : null}
                        <div className="mt-3">
                          <button
                            type="button"
                            className={secondaryButtonClass("sm")}
                            onClick={() => openSelfProposedManager(row)}
                          >
                            {t("capabilities.selfProposedManage")}
                          </button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            ))}
            {selfProposedGroupSections.length === 0 ? (
              <div className="text-xs text-[var(--color-text-muted)]">
                {t("capabilities.selfProposedNoCandidates")}
              </div>
            ) : null}
          </div>
        )}
        </div>
      </section>

      {!selfEvolvingSurface ? (
      <section className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
          <div>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("capabilities.safetyModeTitle")}</div>
            <div className="mt-1 text-xs text-[var(--color-text-muted)]">{t("capabilities.safetyModeHint")}</div>
          </div>
          <div className="text-[11px] text-[var(--color-text-tertiary)]">
            {t("capabilities.safetyModeCurrent", { mode: t(`capabilities.safetyMode.${externalSafetyMode}.label`) })}
          </div>
        </div>
        <div className={settingsWorkspaceBodyClass}>
        <div className="grid gap-2 md:grid-cols-2">
          {(["normal", "conservative"] as ExternalCapabilitySafetyMode[]).map((mode) => {
            const selected = externalSafetyMode === mode;
            return (
              <button
                key={mode}
                type="button"
                className={`rounded-lg border px-3 py-3 text-left ${selected
                  ? "border-emerald-500/30 bg-emerald-500/15"
                  : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)]"} ${busyKey === "policy" ? "opacity-60 cursor-not-allowed" : ""}`}
                disabled={busyKey === "policy" || selected}
                onClick={() => void updateExternalCapabilitySafetyMode(mode)}
              >
                <div className="text-sm font-medium text-[var(--color-text-primary)]">{t(`capabilities.safetyMode.${mode}.label`)}</div>
                <div className="text-xs mt-1 text-[var(--color-text-tertiary)]">{t(`capabilities.safetyMode.${mode}.hint`)}</div>
              </button>
            );
          })}
        </div>
        <div className="text-[11px] mt-2 text-[var(--color-text-muted)]">
          {t("capabilities.safetyModeCurrentRule", { mode: t(`capabilities.safetyMode.${externalSafetyMode}.label`) })}
        </div>
        </div>
      </section>
      ) : null}

      {!selfEvolvingSurface ? (
      <section className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
          <div>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("capabilities.sourcesTitle")}</div>
            <div className="mt-1 text-xs text-[var(--color-text-muted)]">{t("capabilities.sourcesHint")}</div>
          </div>
        </div>
        <div className={settingsWorkspaceBodyClass}>
        <div className="text-[11px] text-[var(--color-text-muted)]">
          {t("capabilities.sourcesSummary", sourceSummary)}
        </div>
        <div className="text-[11px] text-[var(--color-text-muted)]">
          {t("capabilities.levelDistribution", levelDistribution)}
        </div>
        <div className="grid grid-cols-1 gap-2 md:grid-cols-[minmax(0,1fr)_auto] md:items-center">
          <input
            value={sourceQuery}
            onChange={(e) => setSourceQuery(e.target.value)}
            placeholder={t("capabilities.sourceSearchPlaceholder")}
            className={inputClass()}
          />
          <div className="inline-flex rounded-lg border border-[var(--glass-border-subtle)] overflow-hidden">
            <button type="button" onClick={() => setSourceVisibility("all")} className={`px-2.5 py-2 text-xs min-h-[40px] ${sourceVisibility === "all" ? "bg-[var(--glass-tab-bg)] text-[var(--color-text-primary)]" : "bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)]"}`}>{t("capabilities.sourcesVisibilityAll")}</button>
            <button type="button" onClick={() => setSourceVisibility("enabled")} className={`px-2.5 py-2 text-xs min-h-[40px] border-l border-[var(--glass-border-subtle)] ${sourceVisibility === "enabled" ? "bg-[var(--glass-tab-bg)] text-[var(--color-text-primary)]" : "bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)]"}`}>{t("capabilities.sourcesVisibilityEnabled")}</button>
            <button type="button" onClick={() => setSourceVisibility("disabled")} className={`px-2.5 py-2 text-xs min-h-[40px] border-l border-[var(--glass-border-subtle)] ${sourceVisibility === "disabled" ? "bg-[var(--glass-tab-bg)] text-[var(--color-text-primary)]" : "bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)]"}`}>{t("capabilities.sourcesVisibilityDisabled")}</button>
          </div>
        </div>
        <div className="space-y-2">
          {sourceRowsVisible.map((row) => {
            const sid = String(row.source_id || "");
            const enabled = Boolean(row.enabled);
            const rationale = String(sourceRationaleMap.get(sid) || row.rationale || "");
            const syncState = String(row.sync_state || "never");
            const count = Number(row.record_count || 0);
            return (
              <div key={sid} className={settingsWorkspacePanelClass(_isDark)}>
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="text-sm font-medium truncate text-[var(--color-text-primary)]">{sid}</div>
                    <div className="mt-1 flex flex-wrap gap-1">
                      <span className={`px-1.5 py-0.5 rounded text-[10px] ${enabled ? "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400" : "bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]"}`}>{enabled ? t("capabilities.sourceEnabled") : t("capabilities.sourceDisabled")}</span>
                      <span className="px-1.5 py-0.5 rounded text-[10px] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">{t("capabilities.sourceMeta", { level: String(row.source_level || "indexed"), sync: syncState, count })}</span>
                    </div>
                    {rationale ? <div className="text-[11px] mt-1 text-[var(--color-text-tertiary)]">{t("capabilities.sourceRationale", { text: rationale })}</div> : null}
                  </div>
                  <button
                    type="button"
                    className={`px-2.5 py-1.5 rounded text-xs min-h-[32px] ${enabled ? "bg-rose-500/15 text-rose-600 dark:text-rose-400 border border-rose-500/30" : "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 border border-emerald-500/30"} ${busyKey === `source:${sid}` ? "opacity-60 cursor-not-allowed" : ""}`}
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
            <button type="button" className="text-xs text-emerald-600 dark:text-emerald-400" onClick={() => setShowAllSources((v) => !v)}>
              {showAllSources ? t("capabilities.showLessSources") : t("capabilities.showMoreSources", { count: filteredSources.length - SOURCE_PREVIEW_LIMIT })}
            </button>
          ) : null}
        </div>
        </div>
      </section>
      ) : null}

      {!selfEvolvingSurface ? (
      <section className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
          <div>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("capabilities.libraryTitle")}</div>
            <div className="mt-1 text-xs text-[var(--color-text-muted)]">{t("capabilities.libraryHint")}</div>
          </div>
        </div>
        <div className={settingsWorkspaceBodyClass}>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("capabilities.searchPlaceholder")}
          className={inputClass()}
        />
        <div className="grid grid-cols-1 gap-2 md:grid-cols-4">
          <SelectCombobox
            items={[
              { value: "all", label: t("capabilities.filterKindAll") },
              { value: "pack", label: t("capabilities.filterKindPack") },
              { value: "mcp", label: t("capabilities.filterKindMcp") },
              { value: "skill", label: t("capabilities.filterKindSkill") },
            ]}
            value={registryKind}
            onChange={(value) => setRegistryKind(value as RegistryKindFilter)}
            ariaLabel={t("capabilities.filterKindAll")}
            className={inputClass()}
          />
          <SelectCombobox
            items={[
              { value: "all", label: t("capabilities.filterPolicyAll") },
              { value: "actionable", label: t("capabilities.filterPolicyActionable") },
              { value: "blocked", label: t("capabilities.filterPolicyBlocked") },
              { value: "indexed", label: t("capabilities.filterPolicyIndexed") },
            ]}
            value={registryPolicy}
            onChange={(value) => setRegistryPolicy(value as RegistryPolicyFilter)}
            ariaLabel={t("capabilities.filterPolicyAll")}
            className={inputClass()}
          />
          <SelectCombobox
            items={[
              { value: "all", label: t("capabilities.filterSourceAll") },
              ...registrySourceOptions.map((sid) => ({ value: sid, label: sid })),
            ]}
            value={registrySource}
            onChange={setRegistrySource}
            ariaLabel={t("capabilities.filterSourceAll")}
            className={inputClass()}
            searchable
          />
          <div className="grid grid-cols-[auto_minmax(0,1fr)] gap-2 items-center">
            <label className="text-xs text-[var(--color-text-tertiary)]">{t("capabilities.pageSize")}</label>
            <SelectCombobox
              items={REGISTRY_PAGE_SIZE_OPTIONS.map((size) => ({ value: String(size), label: String(size) }))}
              value={String(registryPageSize)}
              onChange={(value) => setRegistryPageSize(Number(value) || 40)}
              ariaLabel={t("capabilities.pageSize")}
              className={inputClass()}
            />
          </div>
        </div>
        <div className="mt-2 text-[11px] text-[var(--color-text-muted)]">
          {t("capabilities.resultsSummary", { count: registryTotalCount })} · {t("capabilities.showingRange", { from: registryRange.from, to: registryRange.to })}
        </div>
        <div ref={registryListRef} className="max-h-[420px] overflow-auto space-y-2">
          {items.map((row) => {
            const capId = String(row.capability_id || "");
            const blockedNow = Boolean(row.blocked_global);
            const isSelfProposed = (
              String(row.source_id || "").trim() === SELF_PROPOSED_SOURCE_ID
              && String(row.kind || "").trim().toLowerCase() === "skill"
            );
            const readinessPreview = normalizeReadinessPreview(row.readiness_preview);
            const recommendationMeta = [
              { label: t("capabilities.useWhen"), value: firstRecommendationLine(row.use_when) },
              { label: t("capabilities.verifyWith"), value: String(row.evidence_kind || "").trim() },
              { label: t("capabilities.gotcha"), value: firstRecommendationLine(row.gotchas) },
              { label: t("capabilities.avoidWhen"), value: firstRecommendationLine(row.avoid_when) },
            ].filter((entry) => entry.value);
            return (
              <div key={capId} className={settingsWorkspacePanelClass(_isDark)}>
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="text-sm font-medium truncate text-[var(--color-text-primary)]">{String(row.name || capId)}</div>
                    <div className="text-[11px] truncate text-[var(--color-text-tertiary)]">{capId}</div>
                    {String(row.description_short || "").trim() ? (
                      <div className="text-[11px] mt-1 text-[var(--color-text-tertiary)]">{String(row.description_short || "")}</div>
                    ) : null}
                    {recommendationMeta.length ? (
                      <div className="mt-1.5 space-y-0.5">
                        {recommendationMeta.map((entry) => (
                          <div key={`${capId}:${entry.label}`} className="text-[10px] leading-4 text-[var(--color-text-muted)]">
                            <span className="font-medium text-[var(--color-text-tertiary)]">{entry.label}: </span>
                            <span>{entry.value}</span>
                          </div>
                        ))}
                      </div>
                    ) : null}
                    <div className="flex flex-wrap gap-1 mt-1">
                      {row.kind ? <span className="px-1.5 py-0.5 rounded text-[10px] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">{row.kind}</span> : null}
                      {row.source_id ? <span className="px-1.5 py-0.5 rounded text-[10px] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">{row.source_id}</span> : null}
                      {row.policy_level ? <span className="px-1.5 py-0.5 rounded text-[10px] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">{row.policy_level}</span> : null}
                      {row.recent_success?.success_count ? <span className="px-1.5 py-0.5 rounded text-[10px] bg-emerald-500/15 text-emerald-600 dark:text-emerald-400">{t("capabilities.recentCount", { count: Number(row.recent_success?.success_count || 0) })}</span> : null}
                      {blockedNow ? <span className="px-1.5 py-0.5 rounded text-[10px] bg-rose-500/15 text-rose-600 dark:text-rose-400">{t("capabilities.blocked")}</span> : null}
                    </div>
                    {renderReadinessPreview(readinessPreview)}
                  </div>
                  <div className="flex items-center gap-1.5 shrink-0">
                    {isSelfProposed ? (
                      <button
                        type="button"
                        className={secondaryButtonClass("sm")}
                        onClick={() => openSelfProposedManager(row)}
                      >
                        {t("capabilities.selfProposedManage")}
                      </button>
                    ) : null}
                    {!isSelfProposed ? (
                      <button
                        type="button"
                        className={`px-2.5 py-1.5 rounded text-xs min-h-[32px] ${blockedNow ? "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 border border-emerald-500/30" : "bg-rose-500/15 text-rose-600 dark:text-rose-400 border border-rose-500/30"} ${busyKey === `block:${capId}` ? "opacity-60 cursor-not-allowed" : ""}`}
                        disabled={busyKey === `block:${capId}`}
                        onClick={() => void toggleBlock(row, !blockedNow)}
                      >
                        {blockedNow ? t("capabilities.unblock") : t("capabilities.block")}
                      </button>
                    ) : null}
                  </div>
                </div>
              </div>
            );
          })}
          {items.length === 0 ? <div className="text-xs text-[var(--color-text-muted)]">{t("capabilities.noLibraryMatches")}</div> : null}
          {registryHasMore ? <div ref={registryLoadMoreRef} className="h-1 w-full" aria-hidden="true" /> : null}
        </div>
        <div className="mt-2 flex items-center justify-between gap-2">
          <div className="text-xs text-[var(--color-text-tertiary)]">
            {loading
              ? t("capabilities.loading")
              : registryHasMore
                ? t("capabilities.showingRange", { from: registryRange.from, to: registryRange.to })
                : t("capabilities.resultsSummary", { count: registryTotalCount })}
          </div>
          {registryHasMore ? (
            <button
              type="button"
              className={secondaryButtonClass("sm")}
              disabled={loading}
              onClick={() => void load({ append: true })}
            >
              {t("capabilities.pageNext")}
            </button>
          ) : null}
        </div>
        </div>
      </section>
      ) : null}

      {!selfEvolvingSurface ? (
      <section className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
          <div>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("capabilities.blockedListTitle")}</div>
            <div className="mt-1 text-xs text-[var(--color-text-muted)]">{t("capabilities.blockedListHint")}</div>
          </div>
        </div>
        <div className={settingsWorkspaceBodyClass}>
        <div className="space-y-2">
          {blocked.length === 0 ? (
            <div className="text-xs text-[var(--color-text-muted)]">{t("capabilities.noBlocked")}</div>
          ) : (
            blocked.map((row) => {
              const capId = String(row.capability_id || "");
              return (
                <div key={capId} className={settingsWorkspacePanelClass(_isDark)}>
                  <div className="flex items-center justify-between gap-2">
                    <code className="text-xs">{capId}</code>
                    <button
                      type="button"
                      className="px-2.5 py-1 rounded text-xs min-h-[30px] bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 border border-emerald-500/30"
                      disabled={busyKey === `block:${capId}`}
                      onClick={() => void toggleBlock(row, false)}
                    >
                      {t("capabilities.unblock")}
                    </button>
                  </div>
                  {String(row.reason || "").trim() ? <div className="text-[11px] mt-1 text-[var(--color-text-tertiary)]">{String(row.reason || "")}</div> : null}
                </div>
              );
            })
          )}
        </div>
        </div>
      </section>
      ) : null}

      {managingCandidate ? (
        <BodyPortal>
        <div
          key={manageCapabilityId}
          ref={manageDialogRef}
          className="fixed inset-0 z-[1000] flex items-end justify-center bg-black/35 px-3 py-4 sm:items-center"
          role="dialog"
          aria-modal="true"
          aria-labelledby="self-proposed-skill-manager-title"
          onPointerDown={closeSelfProposedManager}
        >
          <div
            className="w-full max-w-4xl max-h-[88vh] overflow-hidden rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-elevated)] shadow-2xl"
            onPointerDown={(e) => e.stopPropagation()}
          >
            <div className="flex items-start justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-4 py-3">
              <div className="min-w-0">
                <div id="self-proposed-skill-manager-title" className="text-sm font-semibold text-[var(--color-text-primary)]">
                  {t("capabilities.manageTitle")}
                </div>
                <div className="mt-1 text-xs text-[var(--color-text-muted)]">{t("capabilities.manageSubtitle")}</div>
              </div>
              <button
                type="button"
                className={secondaryButtonClass("sm")}
                onClick={closeSelfProposedManager}
              >
                {t("capabilities.manageClose")}
              </button>
            </div>

            <div className="max-h-[calc(88vh-62px)] overflow-auto px-4 py-4">
              <div className="flex flex-wrap gap-1.5">
                <span className="rounded bg-[var(--glass-tab-bg)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--color-text-secondary)]">
                  {manageCapabilityId}
                </span>
                <span className="rounded bg-[var(--glass-tab-bg)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-secondary)]">
                  {SELF_PROPOSED_SOURCE_ID}
                </span>
                {manageQualificationStatus === "blocked" ? (
                  <span className="rounded bg-rose-500/10 px-1.5 py-0.5 text-[10px] text-rose-600 dark:text-rose-300">
                    {t("capabilities.manageStatusBlocked")}
                  </span>
                ) : null}
              </div>

              {!String(groupId || "").trim() ? (
                <div className="mt-3 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-300">
                  {t("capabilities.manageNoGroupHint")}
                </div>
              ) : null}

              {manageErr ? (
                <div className="mt-3 rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-600 dark:text-rose-400" role="alert">
                  {manageErr}
                </div>
              ) : null}
              {manageNotice ? (
                <div className="mt-3 rounded-lg border border-emerald-500/30 bg-emerald-500/10 px-3 py-2 text-xs text-emerald-700 dark:text-emerald-300" role="status">
                  {manageNotice}
                </div>
              ) : null}

              {manageDuplicateCandidates.length ? (
                <div className="mt-3 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2">
                  <div className="text-xs font-medium text-amber-800 dark:text-amber-200">{t("capabilities.manageDuplicateTitle")}</div>
                  <div className="mt-1 text-[11px] text-amber-700 dark:text-amber-300">{t("capabilities.manageDuplicateHint")}</div>
                  <div className="mt-2 space-y-1">
                    {manageDuplicateCandidates.map((row) => (
                      <div key={String(row.capability_id || "")} className="truncate font-mono text-[11px] text-amber-800 dark:text-amber-200">
                        {String(row.capability_id || "")}
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}

              <div className={`mt-4 ${settingsWorkspacePanelClass(_isDark)}`}>
                <div className="text-xs font-medium text-[var(--color-text-primary)]">{t("capabilities.manageProvenanceTitle")}</div>
                <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">{t("capabilities.manageProvenanceHint")}</div>
                <dl className="mt-3 grid gap-2 sm:grid-cols-2">
                  {manageProvenanceRows.map((row) => (
                    <div key={row.label} className={settingsWorkspaceSoftPanelClass(_isDark)}>
                      <dt className="text-[10px] uppercase tracking-[0.08em] text-[var(--color-text-muted)]">{row.label}</dt>
                      <dd className="mt-1 break-words font-mono text-[11px] text-[var(--color-text-secondary)]">{row.value}</dd>
                    </div>
                  ))}
                </dl>
              </div>

              <div className="mt-4 grid gap-3 md:grid-cols-2">
                <label className="block">
                  <span className="text-xs font-medium text-[var(--color-text-secondary)]">{t("capabilities.manageName")}</span>
                  <input
                    value={manageName}
                    onChange={(e) => setManageName(e.target.value)}
                    className={`mt-1 ${inputClass()}`}
                  />
                </label>
                <label className="block">
                  <span className="text-xs font-medium text-[var(--color-text-secondary)]">{t("capabilities.manageDescription")}</span>
                  <input
                    value={manageDescription}
                    onChange={(e) => setManageDescription(e.target.value)}
                    className={`mt-1 ${inputClass()}`}
                  />
                </label>
              </div>

              <div className="mt-3">
                <label className="block">
                  <span className="text-xs font-medium text-[var(--color-text-secondary)]">{t("capabilities.manageCapsule")}</span>
                  <textarea
                    value={manageCapsuleText}
                    onChange={(e) => setManageCapsuleText(e.target.value)}
                    maxLength={SELF_PROPOSED_CAPSULE_TEXT_MAX}
                    rows={16}
                    className={`mt-1 ${inputClass()} resize-y font-mono text-xs leading-5`}
                  />
                  <span className="mt-1 block text-[10px] text-[var(--color-text-muted)]">
                    {t("capabilities.manageCapsuleLimit", { count: manageCapsuleText.length, max: SELF_PROPOSED_CAPSULE_TEXT_MAX })}
                  </span>
                </label>
              </div>

              <div className="mt-4 flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  className={primaryButtonClass(false)}
                  disabled={busyKey === `manage:${manageCapabilityId}`}
                  onClick={() => void saveManagedSelfProposed()}
                >
                  {busyKey === `manage:${manageCapabilityId}` ? t("common:saving") : t("capabilities.manageSave")}
                </button>
              </div>

              {manageQualificationStatus === "blocked" ? (
                <div className="mt-4 rounded-lg border border-rose-500/20 bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">
                  {t("capabilities.manageBlockedBanner")}
                </div>
              ) : null}

              <div className="mt-4">
                <div className={settingsWorkspacePanelClass(_isDark)}>
                  <div className="text-sm font-medium text-[var(--color-text-primary)]">{t("capabilities.manageRuntimeTitle")}</div>
                  <div className="mt-1 text-xs text-[var(--color-text-muted)]">{t("capabilities.manageAutoloadHint")}</div>
                  <div className={`mt-3 ${settingsWorkspaceSoftPanelClass(_isDark)}`}>
                    <div className="text-xs font-medium text-[var(--color-text-primary)]">{t("capabilities.manageCurrentUseTitle")}</div>
                    <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">{t("capabilities.manageCurrentUseHint")}</div>
                    {manageUsageLoading ? (
                      <div className="mt-2 text-[11px] text-[var(--color-text-tertiary)]">{t("capabilities.manageUsageLoading")}</div>
                    ) : manageUsage?.used ? (
                      <div className="mt-2 space-y-1.5">
                        <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                          {t("capabilities.manageUsageSummary", {
                            active: Number(manageUsage.active_actor_count || 0),
                            startup: Number(manageUsage.startup_autoload_actor_count || 0),
                          })}
                        </div>
                        {manageUsage.group_enabled ? (
                          <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                            {t("capabilities.manageUsageGroup", { count: Number(manageUsage.group_actor_count || 0) })}
                          </div>
                        ) : null}
                        {(manageUsage.session_enabled || []).map((row) => (
                          <div key={`session:${row.actor_id}:${row.expires_at || ""}`} className={settingsWorkspaceSoftPanelClass(_isDark)}>
                            {t("capabilities.manageUsageSession", { actor: capabilityUsageActorLabel(row), ttl: manageUsageTtlLabel(row.ttl_seconds) })}
                          </div>
                        ))}
                        {(manageUsage.actor_enabled || []).map((row) => (
                          <div key={`actor:${row.actor_id}`} className={settingsWorkspaceSoftPanelClass(_isDark)}>
                            {t("capabilities.manageUsageActor", { actor: capabilityUsageActorLabel(row) })}
                          </div>
                        ))}
                        {(manageUsage.actor_autoload || []).map((row) => (
                          <div key={`autoload:${row.actor_id}`} className={settingsWorkspaceSoftPanelClass(_isDark)}>
                            {t("capabilities.manageUsageActorAutoload", { actor: capabilityUsageActorLabel(row) })}
                          </div>
                        ))}
                        {(manageUsage.profile_autoload || []).map((row) => (
                          <div key={`profile:${row.actor_id}:${row.profile_id || ""}`} className={settingsWorkspaceSoftPanelClass(_isDark)}>
                            {t("capabilities.manageUsageProfileAutoload", {
                              actor: capabilityUsageActorLabel(row),
                              profile: String(row.profile_name || row.profile_id || "").trim() || t("capabilities.manageUsageUnknownProfile"),
                            })}
                          </div>
                        ))}
                        {manageUsage.blocked ? (
                          <div className="rounded-md bg-rose-500/10 px-2 py-1.5 text-[11px] text-rose-600 dark:text-rose-300">
                            {t("capabilities.manageUsageBlocked")}
                          </div>
                        ) : null}
                      </div>
                    ) : (
                      <div className={`mt-2 ${settingsWorkspaceSoftPanelClass(_isDark)} text-[11px] text-[var(--color-text-tertiary)]`}>
                        {t("capabilities.manageNoCurrentUse")}
                      </div>
                    )}
                  </div>
                  <div className="mt-4 border-t border-[var(--glass-border-subtle)] pt-3">
                    <div className="text-xs font-medium text-[var(--color-text-primary)]">{t("capabilities.manageActorAssignmentsTitle")}</div>
                    <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">{t("capabilities.manageActorAssignmentsHint")}</div>
                  </div>
                  <div className="mt-3 space-y-2">
                    {manageActors
                      .filter((actor) => String(actor.id || "").trim())
                      .map((actor) => {
                        const actorId = String(actor.id || "").trim();
                        const checked = manageAssignedActorIdSet.has(actorId);
                        const profileInherited = manageProfileActorIdSet.has(actorId);
                        const temporaryActive = manageSessionActorIdSet.has(actorId);
                        const actorScopeActive = manageActorScopeIdSet.has(actorId);
                        const runtimeLabel = [actor.runtime, actor.runner_effective || actor.runner].filter(Boolean).join(" / ");
                        return (
                          <label key={actorId} className={`flex items-start gap-2 ${settingsWorkspaceSoftPanelClass(_isDark)}`}>
                            <input
                              type="checkbox"
                              className="mt-0.5"
                              checked={checked}
                              onChange={() => toggleManagedActorAssignment(actorId)}
                            />
                            <span className="min-w-0 flex-1">
                              <span className="block truncate text-xs font-medium text-[var(--color-text-primary)]">
                                {actor.title ? `${actor.title} (${actorId})` : actorId}
                              </span>
                              {runtimeLabel ? (
                                <span className="mt-0.5 block text-[11px] text-[var(--color-text-tertiary)]">{runtimeLabel}</span>
                              ) : null}
                              <span className="mt-1 flex flex-wrap gap-1">
                                {profileInherited ? (
                                  <span className="rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-700 dark:text-amber-300">
                                    {t("capabilities.manageActorAssignmentProfileBadge")}
                                  </span>
                                ) : null}
                                {temporaryActive ? (
                                  <span className="rounded border border-black/10 bg-[rgb(245,245,245)] px-1.5 py-0.5 text-[10px] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white">
                                    {t("capabilities.manageActorAssignmentTemporaryBadge")}
                                  </span>
                                ) : null}
                                {actorScopeActive ? (
                                  <span className="rounded border border-black/10 bg-[rgb(245,245,245)] px-1.5 py-0.5 text-[10px] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white">
                                    {t("capabilities.manageActorAssignmentActorScopeBadge")}
                                  </span>
                                ) : null}
                              </span>
                            </span>
                          </label>
                        );
                      })}
                    {manageActors.length === 0 ? (
                      <div className={`${settingsWorkspaceSoftPanelClass(_isDark)} text-[11px] text-[var(--color-text-tertiary)]`}>
                        {t("capabilities.manageNoActors")}
                      </div>
                    ) : null}
                  </div>
                  <div className="mt-3 flex justify-end">
                    <button
                      type="button"
                      className={secondaryButtonClass()}
                      disabled={busyKey === `manage-use:${manageCapabilityId}`}
                      onClick={() => void saveManagedActorAssignments()}
                    >
                      {t("capabilities.manageSaveActorAssignments")}
                    </button>
                  </div>
                </div>
              </div>

              <div className={`mt-4 ${settingsWorkspacePanelClass(_isDark)}`}>
                <div className="text-sm font-medium text-[var(--color-text-primary)]">{t("capabilities.manageOtherActionsTitle")}</div>
                <div className="mt-1 text-xs text-[var(--color-text-muted)]">{t("capabilities.manageOtherActionsHint")}</div>
                <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
                  <button
                    type="button"
                    className={`rounded-lg border px-3 py-2 text-xs min-h-[38px] disabled:opacity-60 ${
                      manageQualificationStatus === "blocked"
                        ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                        : "border-rose-500/30 bg-rose-500/10 text-rose-600 dark:text-rose-300"
                    }`}
                    disabled={busyKey === `manage:${manageCapabilityId}`}
                    onClick={() => void saveManagedSelfProposed(
                      manageQualificationStatus === "blocked" ? "qualified" : "blocked",
                      manageQualificationStatus === "blocked" ? "capabilities.manageUnblockedSaved" : "capabilities.manageBlockedSaved",
                    )}
                  >
                    {manageQualificationStatus === "blocked" ? t("capabilities.manageUnblockSkill") : t("capabilities.manageBlockSkill")}
                  </button>
                  <button
                    type="button"
                    className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs min-h-[38px] text-rose-600 dark:text-rose-300 disabled:opacity-60"
                    disabled={busyKey === `manage-remove:${manageCapabilityId}`}
                    onClick={() => void uninstallManagedSelfProposed()}
                  >
                    {t("capabilities.manageRemove")}
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
        </BodyPortal>
      ) : null}
    </div>
  );
}
