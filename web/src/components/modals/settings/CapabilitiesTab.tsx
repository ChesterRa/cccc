import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import { buildCapabilityCenterUrl } from "../../capabilities/capabilityCenterRoute";
import { publishCapabilityChanged } from "../../../utils/capabilityEvents";
import { SlashCommandVisibilityButton } from "./SlashCommandVisibilityButton";
import { SkillAssignmentManagerModal } from "./SkillAssignmentManagerModal";
import {
  Actor,
  CapabilityImportRecord,
  CapabilityOverviewItem,
  CapabilityUsageActorEntry,
  CapabilityUsageSummary,
  GroupMeta,
} from "../../../types";
import {
  canEditSkillRecord,
  canManageSkillAssignments,
  canManageSlashCommandVisibility,
  isCapabilityHiddenFromSlashCommands,
  nextSlashCommandHiddenCapabilities,
} from "./capabilityManagementModel";
import {
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

type ManageQualificationStatus = "qualified" | "blocked";

const SELF_PROPOSED_OVERVIEW_LIMIT = 200;
const SELF_PROPOSED_SOURCE_ID = "agent_self_proposed";
const SELF_PROPOSED_CAPSULE_TEXT_MAX = 2400;

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

function deriveManagedHiddenActorIds(
  actors: Actor[],
  capabilityId: string,
  usage: CapabilityUsageSummary | null,
) {
  const capId = String(capabilityId || "").trim();
  if (!capId) return [];
  const hidden = new Set<string>();
  const actorIds = actors.map((actor) => String(actor.id || "").trim()).filter(Boolean);
  for (const actor of actors) {
    const actorId = String(actor.id || "").trim();
    if (actorId && normalizeCapabilityIdList(actor.capability_hidden).includes(capId)) {
      hidden.add(actorId);
    }
  }
  for (const row of usage?.actor_hidden || []) {
    const actorId = String(row.actor_id || "").trim();
    if (actorId) hidden.add(actorId);
  }
  return actorIds.filter((actorId) => hidden.has(actorId));
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
  const [items, setItems] = useState<CapabilityOverviewItem[]>([]);
  const [groups, setGroups] = useState<GroupMeta[]>([]);
  const [manageCapabilityId, setManageCapabilityId] = useState("");
  const [manageName, setManageName] = useState("");
  const [manageDescription, setManageDescription] = useState("");
  const [manageCapsuleText, setManageCapsuleText] = useState("");
  const [manageQualificationStatus, setManageQualificationStatus] = useState<ManageQualificationStatus>("qualified");
  const [manageQualificationReason, setManageQualificationReason] = useState("");
  const [manageActors, setManageActors] = useState<Actor[]>([]);
  const [manageAssignedActorIds, setManageAssignedActorIds] = useState<string[]>([]);
  const [manageHiddenActorIds, setManageHiddenActorIds] = useState<string[]>([]);
  const [slashHiddenCapabilityIds, setSlashHiddenCapabilityIds] = useState<string[]>([]);
  const [manageUsage, setManageUsage] = useState<CapabilityUsageSummary | null>(null);
  const [manageUsageLoading, setManageUsageLoading] = useState(false);
  const overviewRequestSeqRef = useRef(0);
  const failedLoadTextRef = useRef("");

  failedLoadTextRef.current = t("capabilities.failedLoad");

  const closeSelfProposedManager = useCallback(() => {
    setManageCapabilityId("");
    setManageAssignedActorIds([]);
    setManageHiddenActorIds([]);
    setManageUsage(null);
    setManageUsageLoading(false);
    setManageErr("");
    setManageNotice("");
  }, []);

  const load = useCallback(async () => {
    if (!isActive) return;
    const requestSeq = overviewRequestSeqRef.current + 1;
    overviewRequestSeqRef.current = requestSeq;
    setLoading(true);
    setErr("");
    try {
      const [overviewResp, groupsResp, stateResp] = await Promise.all([
        api.fetchCapabilityOverview({
          includeIndexed: true,
          includeSourceInstances: false,
          limit: SELF_PROPOSED_OVERVIEW_LIMIT,
          offset: 0,
          kind: "skill",
          policy: "all",
          sourceId: SELF_PROPOSED_SOURCE_ID,
        }),
        selfEvolvingSurface ? Promise.resolve(null) : api.fetchGroups(),
        selfEvolvingSurface || !String(groupId || "").trim()
          ? Promise.resolve(null)
          : api.fetchSlashCommandCapabilityState(String(groupId || "").trim(), "user", { noCache: true }),
      ]);
      if (overviewRequestSeqRef.current != requestSeq) return;
      if (!overviewResp.ok) {
        setErr(overviewResp.error?.message || failedLoadTextRef.current);
        setItems([]);
        setGroups([]);
      } else {
        const nextItems = Array.isArray(overviewResp.result?.items) ? overviewResp.result.items : [];
        setItems(nextItems);
      }
      if (groupsResp?.ok) {
        setGroups(Array.isArray(groupsResp.result?.groups) ? groupsResp.result.groups : []);
      } else if (!selfEvolvingSurface) {
        setGroups([]);
      }
      if (stateResp?.ok) {
        setSlashHiddenCapabilityIds(normalizeCapabilityIdList(stateResp.result?.actor_hidden_capabilities));
      } else if (!selfEvolvingSurface) {
        setSlashHiddenCapabilityIds([]);
      }
    } catch (e) {
      if (overviewRequestSeqRef.current != requestSeq) return;
      setErr(e instanceof Error ? e.message : failedLoadTextRef.current);
      setItems([]);
      setGroups([]);
    } finally {
      if (overviewRequestSeqRef.current === requestSeq) {
        setLoading(false);
      }
    }
  }, [groupId, isActive, selfEvolvingSurface]);

  useEffect(() => {
    if (!isActive) return;
    void load();
  }, [isActive, load]);

  const selfProposedCandidates = useMemo(() => {
    const gid = String(groupId || "").trim();
    return items.filter((row) => (
      String(row.source_id || "").trim() === SELF_PROPOSED_SOURCE_ID
      && String(row.kind || "").trim().toLowerCase() === "skill"
      && (!selfEvolvingSurface || !gid || String(row.origin_group_id || "").trim() === gid)
    ));
  }, [groupId, items, selfEvolvingSurface]);

  const manageableSkillCandidates = useMemo(() => {
    const byId = new Map<string, CapabilityOverviewItem>();
    for (const row of items) {
      const capId = String(row.capability_id || "").trim();
      if (!capId || !canManageSkillAssignments(row)) continue;
      byId.set(capId, row);
    }
    return Array.from(byId.values());
  }, [items]);

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
    return manageableSkillCandidates.find((row) => String(row.capability_id || "").trim() === manageCapabilityId) || null;
  }, [manageCapabilityId, manageableSkillCandidates]);

  const managingCandidateEditable = useMemo(() => {
    return managingCandidate ? canEditSkillRecord(managingCandidate) : false;
  }, [managingCandidate]);

  const manageDuplicateCandidates = useMemo(() => {
    if (!managingCandidate || !managingCandidateEditable) return [];
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
  }, [manageCapabilityId, managingCandidate, managingCandidateEditable, selfProposedCandidates]);

  const manageUsageTtlLabel = useCallback((seconds?: number) => {
    const safeSeconds = Number.isFinite(Number(seconds)) ? Math.max(0, Math.trunc(Number(seconds))) : 0;
    if (safeSeconds < 60) return t("capabilities.manageUsageTtlSeconds");
    if (safeSeconds < 3600) return t("capabilities.manageUsageTtlMinutes", { count: Math.ceil(safeSeconds / 60) });
    return t("capabilities.manageUsageTtlHours", { count: Math.ceil(safeSeconds / 3600) });
  }, [t]);

  const manageAssignedActorIdSet = useMemo(() => new Set(manageAssignedActorIds), [manageAssignedActorIds]);
  const manageHiddenActorIdSet = useMemo(() => new Set(manageHiddenActorIds), [manageHiddenActorIds]);

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

  const toggleSlashCommandVisibility = async (row: CapabilityOverviewItem, nextVisible: boolean) => {
    const gid = String(groupId || "").trim();
    const capId = String(row.capability_id || "").trim();
    if (!gid) {
      setErr(t("capabilities.requireGroup"));
      return;
    }
    if (!capId || !canManageSkillAssignments(row)) return;
    const nextHidden = nextSlashCommandHiddenCapabilities(slashHiddenCapabilityIds, capId, nextVisible);
    setBusyKey(`slash-visible:${capId}`);
    setErr("");
    setSlashHiddenCapabilityIds(nextHidden);
    try {
      const resp = await api.updateGroupCapabilityVisibility(gid, capId, {
        actorId: "user",
        hidden: !nextVisible,
        reason: "web_capabilities_slash_visibility",
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilities.failedSlashVisibility"));
        await refreshSlashHiddenState();
        return;
      }
      const stateResp = await api.fetchSlashCommandCapabilityState(gid, "user", { noCache: true });
      if (stateResp.ok) {
        setSlashHiddenCapabilityIds(normalizeCapabilityIdList(stateResp.result?.actor_hidden_capabilities));
      }
      publishCapabilityChanged(gid);
    } catch (e) {
      setErr(e instanceof Error ? e.message : t("capabilities.failedSlashVisibility"));
      await refreshSlashHiddenState();
    } finally {
      setBusyKey("");
    }
  };

  const refreshSlashHiddenState = async () => {
    const gid = String(groupId || "").trim();
    if (!gid) {
      setSlashHiddenCapabilityIds([]);
      return;
    }
    const resp = await api.fetchSlashCommandCapabilityState(gid, "user", { noCache: true });
    if (resp.ok) {
      setSlashHiddenCapabilityIds(normalizeCapabilityIdList(resp.result?.actor_hidden_capabilities));
    }
  };

  const refreshManageAssignmentState = async (capabilityId: string = manageCapabilityId) => {
    const gid = String(groupId || "").trim();
    const capId = String(capabilityId || "").trim();
    if (!gid) {
      setManageActors([]);
      setManageAssignedActorIds([]);
      setManageHiddenActorIds([]);
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
      setManageHiddenActorIds(deriveManagedHiddenActorIds(actors, capId, usage));
      if (!actorsResp.ok) {
        setManageErr(actorsResp.error?.message || t("capabilities.manageActorLoadFailed"));
      } else if (usageResp && !usageResp.ok) {
        setManageErr(usageResp.error?.message || t("capabilities.manageUsageLoadFailed"));
      }
    } catch (e) {
      setManageActors([]);
      setManageAssignedActorIds([]);
      setManageHiddenActorIds([]);
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

  const openSkillAssignmentManager = (row: CapabilityOverviewItem) => {
    const capId = String(row.capability_id || "").trim();
    if (!capId) return;
    setManageCapabilityId(capId);
    setManageName(String(row.name || capId));
    setManageDescription(String(row.description_short || ""));
    setManageCapsuleText(canEditSkillRecord(row)
      ? String(row.capsule_text || "").trim() || selfProposedFallbackCapsule(row)
      : "");
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

  const toggleManagedActorVisibility = (actorId: string) => {
    const aid = String(actorId || "").trim();
    if (!aid) return;
    setManageHiddenActorIds((current) => {
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
      const hiddenDesired = new Set(manageHiddenActorIds.map((item) => String(item || "").trim()).filter(Boolean));
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
        const currentHidden = normalizeCapabilityIdList(actor.capability_hidden);
        const hasAutoload = currentAutoload.includes(capId);
        const hasHidden = currentHidden.includes(capId);
        const shouldAutoload = desired.has(aid);
        const shouldHide = hiddenDesired.has(aid);
        const nextHidden = shouldHide
          ? (hasHidden ? currentHidden : [...currentHidden, capId])
          : currentHidden.filter((item) => item !== capId);
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
              capabilityHidden: nextHidden,
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
          } else if (hasHidden !== shouldHide) {
            const resp = await api.updateActor(gid, aid, undefined, undefined, undefined, undefined, {
              capabilityHidden: nextHidden,
            });
            if (!resp.ok) {
              setManageErr(resp.error?.message || t("capabilities.manageActorAssignmentsFailed"));
              return;
            }
          }
        } else {
          if (hasAutoload) {
            const resp = await api.updateActor(gid, aid, undefined, undefined, undefined, undefined, {
              capabilityAutoload: currentAutoload.filter((item) => item !== capId),
              capabilityHidden: nextHidden,
            });
            if (!resp.ok) {
              setManageErr(resp.error?.message || t("capabilities.manageActorAssignmentsFailed"));
              return;
            }
          } else if (hasHidden !== shouldHide) {
            const resp = await api.updateActor(gid, aid, undefined, undefined, undefined, undefined, {
              capabilityHidden: nextHidden,
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

  const renderSlashVisibilityControl = (row: CapabilityOverviewItem) => {
    if (!canManageSlashCommandVisibility(row)) return null;
    const capId = String(row.capability_id || "").trim();
    if (!capId) return null;
    const hidden = isCapabilityHiddenFromSlashCommands(capId, slashHiddenCapabilityIds);
    return (
      <SlashCommandVisibilityButton
        hidden={hidden}
        busy={busyKey === `slash-visible:${capId}`}
        visibleLabel={t("capabilities.slashCommandVisible")}
        hiddenLabel={t("capabilities.slashCommandHidden")}
        showActionLabel={t("capabilities.showInSlashCommands")}
        hideActionLabel={t("capabilities.hideFromSlashCommands")}
        onToggle={(nextVisible) => void toggleSlashCommandVisibility(row, nextVisible)}
      />
    );
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
            <div className="flex flex-wrap items-center justify-end gap-2">
              <button
                type="button"
                className={secondaryButtonClass("sm")}
                onClick={() => {
                  window.open(buildCapabilityCenterUrl(groupId), "_blank", "noopener,noreferrer");
                }}
              >
                {t("capabilities.openCenter")}
              </button>
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
                  <div className="mt-3 flex flex-wrap items-center gap-2">
                    {renderSlashVisibilityControl(row)}
                    <button
                      type="button"
                      className={secondaryButtonClass("sm")}
                      onClick={() => openSkillAssignmentManager(row)}
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
                        <div className="mt-3 flex flex-wrap items-center gap-2">
                          {renderSlashVisibilityControl(row)}
                          <button
                            type="button"
                            className={secondaryButtonClass("sm")}
                            onClick={() => openSkillAssignmentManager(row)}
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

      {managingCandidate ? (
        <SkillAssignmentManagerModal
          isDark={_isDark}
          candidate={managingCandidate}
          editable={managingCandidateEditable}
          capabilityId={manageCapabilityId}
          groupId={groupId}
          name={manageName}
          description={manageDescription}
          capsuleText={manageCapsuleText}
          capsuleTextMax={SELF_PROPOSED_CAPSULE_TEXT_MAX}
          qualificationStatus={manageQualificationStatus}
          error={manageErr}
          notice={manageNotice}
          duplicateCandidates={manageDuplicateCandidates}
          provenanceRows={manageProvenanceRows}
          usage={manageUsage}
          usageLoading={manageUsageLoading}
          actors={manageActors}
          assignedActorIds={manageAssignedActorIdSet}
          hiddenActorIds={manageHiddenActorIdSet}
          profileActorIds={manageProfileActorIdSet}
          sessionActorIds={manageSessionActorIdSet}
          actorScopeIds={manageActorScopeIdSet}
          busyKey={busyKey}
          labels={{
            title: t(managingCandidateEditable ? "capabilities.manageTitle" : "capabilities.manageAssignmentsTitle"),
            subtitle: t(managingCandidateEditable ? "capabilities.manageSubtitle" : "capabilities.manageAssignmentsSubtitle"),
            close: t("capabilities.manageClose"),
            statusBlocked: t("capabilities.manageStatusBlocked"),
            noGroupHint: t("capabilities.manageNoGroupHint"),
            duplicateTitle: t("capabilities.manageDuplicateTitle"),
            duplicateHint: t("capabilities.manageDuplicateHint"),
            provenanceTitle: t("capabilities.manageProvenanceTitle"),
            provenanceHint: t("capabilities.manageProvenanceHint"),
            name: t("capabilities.manageName"),
            description: t("capabilities.manageDescription"),
            capsule: t("capabilities.manageCapsule"),
            capsuleLimit: t("capabilities.manageCapsuleLimit", { count: manageCapsuleText.length, max: SELF_PROPOSED_CAPSULE_TEXT_MAX }),
            save: t("capabilities.manageSave"),
            saving: t("common:saving"),
            blockedBanner: t("capabilities.manageBlockedBanner"),
            runtimeTitle: t("capabilities.manageRuntimeTitle"),
            autoloadHint: t("capabilities.manageAutoloadHint"),
            currentUseTitle: t("capabilities.manageCurrentUseTitle"),
            currentUseHint: t("capabilities.manageCurrentUseHint"),
            usageLoading: t("capabilities.manageUsageLoading"),
            usageSummary: t("capabilities.manageUsageSummary", {
              active: Number(manageUsage?.active_actor_count || 0),
              startup: Number(manageUsage?.startup_autoload_actor_count || 0),
            }),
            usageGroup: t("capabilities.manageUsageGroup", { count: Number(manageUsage?.group_actor_count || 0) }),
            usageSession: (row) => t("capabilities.manageUsageSession", { actor: capabilityUsageActorLabel(row), ttl: manageUsageTtlLabel(row.ttl_seconds) }),
            usageActor: (row) => t("capabilities.manageUsageActor", { actor: capabilityUsageActorLabel(row) }),
            usageActorAutoload: (row) => t("capabilities.manageUsageActorAutoload", { actor: capabilityUsageActorLabel(row) }),
            usageProfileAutoload: (row) => t("capabilities.manageUsageProfileAutoload", {
              actor: capabilityUsageActorLabel(row),
              profile: String(row.profile_name || row.profile_id || "").trim() || t("capabilities.manageUsageUnknownProfile"),
            }),
            usageActorHidden: (row) => t("capabilities.manageUsageActorHidden", { actor: capabilityUsageActorLabel(row) }),
            usageBlocked: t("capabilities.manageUsageBlocked"),
            noCurrentUse: t("capabilities.manageNoCurrentUse"),
            actorAssignmentsTitle: t("capabilities.manageActorAssignmentsTitle"),
            actorAssignmentsHint: t("capabilities.manageActorAssignmentsHint"),
            profileBadge: t("capabilities.manageActorAssignmentProfileBadge"),
            temporaryBadge: t("capabilities.manageActorAssignmentTemporaryBadge"),
            actorScopeBadge: t("capabilities.manageActorAssignmentActorScopeBadge"),
            hiddenBadge: t("capabilities.manageActorAssignmentHiddenBadge"),
            noActors: t("capabilities.manageNoActors"),
            hideInMenus: t("capabilities.manageHideInMenus"),
            saveActorAssignments: t("capabilities.manageSaveActorAssignments"),
            otherActionsTitle: t("capabilities.manageOtherActionsTitle"),
            otherActionsHint: t("capabilities.manageOtherActionsHint"),
            unblockSkill: t("capabilities.manageUnblockSkill"),
            blockSkill: t("capabilities.manageBlockSkill"),
            remove: t("capabilities.manageRemove"),
          }}
          onClose={closeSelfProposedManager}
          onNameChange={setManageName}
          onDescriptionChange={setManageDescription}
          onCapsuleTextChange={setManageCapsuleText}
          onSaveRecord={() => void saveManagedSelfProposed()}
          onToggleRecordBlock={() => void saveManagedSelfProposed(
            manageQualificationStatus === "blocked" ? "qualified" : "blocked",
            manageQualificationStatus === "blocked" ? "capabilities.manageUnblockedSaved" : "capabilities.manageBlockedSaved",
          )}
          onRemoveRecord={() => void uninstallManagedSelfProposed()}
          onToggleActor={toggleManagedActorAssignment}
          onToggleActorVisibility={toggleManagedActorVisibility}
          onSaveActorAssignments={() => void saveManagedActorAssignments()}
        />
      ) : null}
    </div>
  );
}
