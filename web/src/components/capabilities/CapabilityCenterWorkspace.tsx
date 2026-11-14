import { useCallback, useEffect, useMemo, useState } from "react";
import type React from "react";
import {
  BookOpen,
  Boxes,
  ChevronLeft,
  Eye,
  EyeOff,
  ListChecks,
  Plug,
  Power,
  PowerOff,
  RefreshCcw,
  Search,
  Shield,
  SlidersHorizontal,
  Trash2,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { useDebouncedValue } from "../../hooks/useDebouncedValue";
import { useModalA11y } from "../../hooks/useModalA11y";
import { normalizeLanguageCode } from "../../i18n/languages";
import * as api from "../../services/api";
import { publishCapabilityChanged } from "../../utils/capabilityEvents";
import { formatFullTime, formatMessageTimestamp } from "../../utils/time";
import { HoverTooltip } from "../HoverTooltip";
import { Button } from "../ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";
import { cn } from "../../lib/utils";
import type {
  CapabilityDomain,
  CapabilityMemberProjection,
  CapabilityOverviewItem,
  CapabilitySourceState,
  CapabilityStateResult,
} from "../../types";
import { CapabilityControlsPanel } from "./CapabilityControlsPanel";
import {
  CapabilityRecommendedView,
  RecommendationDotIcon,
  RecommendationVerificationLabel,
} from "./CapabilityRecommendedView";
import { SourcesSummary, SourcesView } from "./CapabilitySourcesView";
import {
  RECOMMENDATION_REASON_CODES,
  RECOMMENDATION_SOURCE_ID,
  firstRecommendation,
  isReleaseRecommendation,
  recommendationAuthKey,
  recommendationBinding,
  recommendationCanEnableForGroup,
  recommendationCount,
  recommendationDate,
  recommendationDot,
  recommendationMemberDot,
  recommendationRows,
  recommendationRunsOnKey,
  recommendationSourceLabel,
  recommendationVerification,
} from "./recommendationModel";
import {
  capabilityCenterDescription,
  capabilityCenterDisplayName,
  capabilityCenterEnabledIds,
  capabilityCenterFilterItemsForSystemVisibility,
  capabilityCenterFrameClass,
  capabilityCenterHiddenIds,
  capabilityCenterIsBlocked,
  capabilityCenterNeedsSetup,
  capabilityCenterPageRange,
  capabilityCenterPaginationMode,
  capabilityCenterRemovalAction,
  capabilityCenterRootClass,
  capabilityCenterSectionTypeFilter,
  capabilityCenterSourceRemovalAction,
  capabilityCenterTurnOff,
  capabilityCenterType,
  capabilityCenterTypeLabel,
  CAPABILITY_CENTER_DEFAULT_PAGE_SIZE,
  CAPABILITY_CENTER_PAGE_SIZE_OPTIONS,
  filterCapabilityCenterItems,
  filterCapabilityCenterRemovedItems,
  mergeCapabilityCenterStickyItems,
  normalizeCapabilityCenterPagination,
  summarizeCapabilityCenter,
  type CapabilityCenterSection,
  type CapabilityCenterStateFilter,
} from "./capabilityCenterModel";
import {
  canManageSlashCommandVisibility,
  isCapabilityHiddenFromSlashCommands,
} from "../modals/settings/capabilityManagementModel";
import { useCapabilityCenterData } from "./useCapabilityCenterData";
import { LcdIcon } from "../Icons";
import type { LcdGlyphName } from "../LcdGlyph";

interface CapabilityCenterWorkspaceProps {
  isOpen: boolean;
  onClose?: () => void;
  groupId?: string;
  isDark?: boolean;
}

const stateFilters: Array<{ id: CapabilityCenterStateFilter; label: string }> = [
  { id: "all", label: "All states" },
  { id: "enabled", label: "Enabled" },
  { id: "slash_visible", label: "/ visible" },
  { id: "slash_hidden", label: "/ hidden" },
  { id: "blocked", label: "Blocked" },
  { id: "needs_setup", label: "Needs setup" },
];

const CAPABILITY_CENTER_QUERY_DEBOUNCE_MS = 200;

/** Update failures the user can act on, said in their language. */
const UPDATE_ERROR_CODES: ReadonlySet<string> = new Set([
  "capability_not_installed",
  "capability_recipe_conflict",
  "capability_update_unsupported",
]);
/** What a finished update did, and why an update cannot be offered here. */
const UPDATE_OUTCOMES: ReadonlySet<string> = new Set(["configured", "ready", "unchanged"]);
const UPDATE_UNAVAILABLE: ReadonlySet<string> = new Set([
  "delivery_form_not_supported",
  "credential_route_not_supported",
]);

type UpdateOutcome = { capabilityId: string; state: string };

type CapabilityCenterConfirmDialogState = {
  title: string;
  description: string;
  confirmLabel: string;
  tone?: "default" | "destructive";
  onConfirm: () => Promise<void> | void;
};

const sectionItems: Array<{
  id: CapabilityCenterSection;
  icon: typeof BookOpen;
  glyph: LcdGlyphName;
  label: string;
  hint: string;
}> = [
  {
    id: "recommended",
    icon: ListChecks,
    glyph: "list-checks",
    label: "Recommended",
    hint: "Grouped by the work members do",
  },
  { id: "skill", icon: BookOpen, glyph: "book-open", label: "Skills", hint: "Procedural skills" },
  { id: "mcp", icon: Plug, glyph: "plug", label: "MCP", hint: "Tool servers" },
  { id: "sources", icon: Boxes, glyph: "package", label: "Sources", hint: "Capability origins" },
];

function statusBadgeClass(kind: "neutral" | "good" | "warn" | "danger") {
  if (kind === "good")
    return "border-emerald-500/25 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
  if (kind === "warn")
    return "border-amber-500/28 bg-amber-500/12 text-amber-700 dark:text-amber-300";
  if (kind === "danger")
    return "border-rose-500/28 bg-rose-500/12 text-rose-700 dark:text-rose-300";
  return "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]";
}

// The LCD look reads a badge's meaning through the shared status hooks.
function statusBadgeTone(kind: "neutral" | "good" | "warn" | "danger") {
  if (kind === "good") return "ok";
  if (kind === "warn") return "warning";
  if (kind === "danger") return "error";
  return undefined;
}

function MiniBadge({
  children,
  kind = "neutral",
  title,
}: {
  children: React.ReactNode;
  kind?: "neutral" | "good" | "warn" | "danger";
  title?: string;
}) {
  return (
    <span
      className={`inline-flex min-h-6 max-w-full items-center rounded border px-2 text-[11px] font-medium ${statusBadgeClass(kind)}`}
      data-status-tone={statusBadgeTone(kind)}
      title={title}
    >
      {children}
    </span>
  );
}

function TooltipIconButton({
  label,
  children,
}: {
  label: React.ReactNode;
  children: (
    referenceProps: Record<string, unknown>,
    setReference: (node: HTMLElement | null) => void,
  ) => React.ReactNode;
}) {
  return (
    <HoverTooltip label={label}>
      {(getReferenceProps, setReference) =>
        children(getReferenceProps({ className: "inline-flex" }), setReference)
      }
    </HoverTooltip>
  );
}

export function CapabilityCenterWorkspace({
  isOpen,
  onClose,
  groupId = "",
  isDark: _isDark,
}: CapabilityCenterWorkspaceProps) {
  const { t, i18n } = useTranslation("settings");
  // Escape closes only the top layer: a confirmation first, then the center, then the panel below.
  const closeCenter = useCallback(() => onClose?.(), [onClose]);
  const { modalRef } = useModalA11y(isOpen, closeCenter);
  const locale = normalizeLanguageCode(i18n.resolvedLanguage || i18n.language);
  const [section, setSection] = useState<CapabilityCenterSection>("recommended");
  // The recommendations keep their own selection; on a narrow screen a pick opens its detail.
  const [recommendedId, setRecommendedId] = useState("");
  const [recommendedDetailOpen, setRecommendedDetailOpen] = useState(false);
  const [groupTitle, setGroupTitle] = useState("");
  const [query, setQuery] = useState("");
  const debouncedQuery = useDebouncedValue(query.trim(), CAPABILITY_CENTER_QUERY_DEBOUNCE_MS);
  const [stateFilter, setStateFilter] = useState<CapabilityCenterStateFilter>("all");
  const [showSystem, setShowSystem] = useState(false);
  const [stickyItems, setStickyItems] = useState<CapabilityOverviewItem[]>([]);
  const [removedCapabilityIds, setRemovedCapabilityIds] = useState<Set<string>>(() => new Set());
  const [pageIndex, setPageIndex] = useState(0);
  const [pageSize, setPageSize] = useState(CAPABILITY_CENTER_DEFAULT_PAGE_SIZE);
  const [busyKey, setBusyKey] = useState("");
  // What the last update did, said under that capability until another update.
  const [updateOutcome, setUpdateOutcome] = useState<UpdateOutcome | null>(null);
  const [mobileControlsOpen, setMobileControlsOpen] = useState(false);
  const [confirmDialog, setConfirmDialog] = useState<CapabilityCenterConfirmDialogState | null>(
    null,
  );
  const localizedSections = useMemo(
    () =>
      sectionItems.map((item) => ({
        ...item,
        label: t(`capabilityCenter.sections.${item.id}.label`),
        hint: t(`capabilityCenter.sections.${item.id}.hint`),
      })),
    [t],
  );
  const localizedStateFilters = useMemo(
    () =>
      stateFilters.map((item) => ({
        ...item,
        label: t(`capabilityCenter.stateOptions.${item.id}`),
      })),
    [t],
  );

  const listSection = section === "skill" || section === "mcp";
  const scopedStateFilter = listSection ? stateFilter : "all";
  const scopedTypeFilter = capabilityCenterSectionTypeFilter(section);
  const paginationMode = useMemo(
    () => capabilityCenterPaginationMode({ section, stateFilter: scopedStateFilter }),
    [section, scopedStateFilter],
  );
  const clientPagination = paginationMode === "client";
  const {
    domains,
    error: err,
    hasMore,
    items,
    loadState,
    loading,
    matchedQuery,
    refresh,
    selectedId,
    setError: setErr,
    setSelectedId,
    sources,
    state,
    summaryStats,
    totalCount,
  } = useCapabilityCenterData({
    clientPagination,
    failedLoadMessage: t("capabilityCenter.failedLoad"),
    failedStateMessage: t("capabilityCenter.failedLoadState"),
    groupId,
    isOpen,
    locale,
    pageIndex,
    pageSize,
    query: listSection ? debouncedQuery : "",
    sourceId: section === "recommended" ? RECOMMENDATION_SOURCE_ID : undefined,
    stateFilter: scopedStateFilter,
    typeFilter: scopedTypeFilter,
  });
  const recommendedRows = useMemo(
    () => (section === "recommended" ? recommendationRows(items, domains) : []),
    [domains, items, section],
  );
  const recommendedSelected = useMemo(() => {
    if (section !== "recommended") return null;
    const id = items.some((item) => item.capability_id === recommendedId)
      ? recommendedId
      : firstRecommendation(recommendedRows);
    return items.find((item) => item.capability_id === id) || null;
  }, [items, recommendedId, recommendedRows, section]);

  useEffect(() => {
    setRecommendedDetailOpen(false);
  }, [section]);

  // The page names the Group its Group-scoped actions apply to.
  useEffect(() => {
    const gid = String(groupId || "").trim();
    setGroupTitle("");
    if (!isOpen || !gid) return;
    let current = true;
    void api.fetchGroup(gid).then((response) => {
      if (current && response.ok) setGroupTitle(String(response.result.group?.title || "").trim());
    });
    return () => {
      current = false;
    };
  }, [groupId, isOpen]);
  const hiddenIds = useMemo(() => capabilityCenterHiddenIds(state), [state]);
  const loadedStats = useMemo(() => summarizeCapabilityCenter(items, state), [items, state]);
  const stats = useMemo(
    () =>
      summaryStats
        ? { ...summaryStats, enabled: loadedStats.enabled, slashHidden: loadedStats.slashHidden }
        : loadedStats,
    [loadedStats, summaryStats],
  );
  const filteredItems = useMemo(
    () =>
      filterCapabilityCenterRemovedItems(
        filterCapabilityCenterItems(items, {
          query,
          matchedQuery,
          typeFilter: scopedTypeFilter,
          stateFilter: scopedStateFilter,
          state,
        }),
        removedCapabilityIds,
      ),
    [items, matchedQuery, query, removedCapabilityIds, scopedStateFilter, scopedTypeFilter, state],
  );
  const baseMatchingItems = useMemo(
    () =>
      capabilityCenterFilterItemsForSystemVisibility(filteredItems, {
        showSystem,
        query,
        matchedQuery,
        state,
      }),
    [filteredItems, matchedQuery, query, showSystem, state],
  );
  const stickyMatchingItems = useMemo(
    () =>
      capabilityCenterFilterItemsForSystemVisibility(
        filterCapabilityCenterItems(stickyItems, {
          query,
          matchedQuery,
          typeFilter: scopedTypeFilter,
          stateFilter: "all",
          state,
        }),
        { showSystem, query, matchedQuery, state },
      ),
    [matchedQuery, query, scopedTypeFilter, showSystem, state, stickyItems],
  );
  const matchingItems = useMemo(
    () => mergeCapabilityCenterStickyItems(baseMatchingItems, stickyMatchingItems),
    [baseMatchingItems, stickyMatchingItems],
  );
  const effectiveTotalCount = clientPagination ? matchingItems.length : totalCount;
  const normalizedPage = useMemo(
    () =>
      normalizeCapabilityCenterPagination({ pageIndex, pageSize, totalCount: effectiveTotalCount }),
    [effectiveTotalCount, pageIndex, pageSize],
  );
  const visibleItems = useMemo(
    () =>
      clientPagination
        ? matchingItems.slice(
            normalizedPage.offset,
            normalizedPage.offset + normalizedPage.pageSize,
          )
        : matchingItems,
    [clientPagination, matchingItems, normalizedPage.offset, normalizedPage.pageSize],
  );
  const selected = useMemo(
    () =>
      visibleItems.find((item) => String(item.capability_id || "") === selectedId) ||
      visibleItems[0] ||
      null,
    [selectedId, visibleItems],
  );
  const pageRange = useMemo(
    () =>
      capabilityCenterPageRange({
        pageIndex: normalizedPage.pageIndex,
        pageSize: normalizedPage.pageSize,
        itemCount: visibleItems.length,
        totalCount: effectiveTotalCount,
      }),
    [effectiveTotalCount, normalizedPage.pageIndex, normalizedPage.pageSize, visibleItems.length],
  );

  useEffect(() => {
    setPageIndex(0);
  }, [query, section, stateFilter]);

  useEffect(() => {
    setStickyItems([]);
  }, [pageIndex, pageSize, query, section, showSystem, stateFilter]);

  const rememberStickyItem = useCallback((row: CapabilityOverviewItem) => {
    const capId = String(row.capability_id || "").trim();
    if (!capId) return;
    setStickyItems((current) =>
      current.filter((item) => String(item.capability_id || "").trim() !== capId).concat(row),
    );
  }, []);

  const toggleSlashVisibility = useCallback(
    async (row: CapabilityOverviewItem) => {
      const capId = String(row.capability_id || "").trim();
      if (!groupId || !capId || !canManageSlashCommandVisibility(row)) return;
      const hidden = isCapabilityHiddenFromSlashCommands(capId, hiddenIds);
      setBusyKey(`slash:${capId}`);
      setErr("");
      try {
        const resp = await api.updateGroupCapabilityVisibility(groupId, capId, {
          hidden: !hidden,
          actorId: "user",
          reason: "capability center slash command visibility",
        });
        if (!resp.ok) {
          setErr(resp.error?.message || t("capabilityCenter.failedSlashVisibility"));
          return;
        }
        rememberStickyItem(row);
        // The same read as every other state reload, so a failed or unusable read is reported.
        await loadState();
        publishCapabilityChanged(groupId);
      } finally {
        setBusyKey("");
      }
    },
    [groupId, hiddenIds, loadState, rememberStickyItem, setErr, t],
  );

  const executeRemoveCapability = useCallback(
    async (
      row: CapabilityOverviewItem,
      action: ReturnType<typeof capabilityCenterRemovalAction>,
    ) => {
      const capId = String(row.capability_id || "").trim();
      if (!groupId || !capId || action === "none") return;
      setBusyKey(`remove:${capId}`);
      setErr("");
      try {
        const resp = await api.uninstallCapability(groupId, capId, {
          actorId: "user",
          reason: `capability center ${action}`,
        });
        if (!resp.ok) {
          setErr(resp.error?.message || t("capabilityCenter.remove.failed"));
          return;
        }
        setRemovedCapabilityIds((current) => new Set([...current, capId]));
        await refresh();
        publishCapabilityChanged(groupId);
      } finally {
        setBusyKey("");
      }
    },
    [groupId, refresh, setErr, t],
  );

  const removeCapability = useCallback(
    (row: CapabilityOverviewItem, action: ReturnType<typeof capabilityCenterRemovalAction>) => {
      const capId = String(row.capability_id || "").trim();
      if (!groupId || !capId || action === "none") return;
      setConfirmDialog({
        title: t(`capabilityCenter.remove.confirmTitle.${action}`, {
          name: capabilityCenterDisplayName(row),
          id: capId,
        }),
        description: t(`capabilityCenter.remove.confirmDescription.${action}`, {
          name: capabilityCenterDisplayName(row),
          id: capId,
        }),
        confirmLabel: t(`capabilityCenter.remove.label.${action}`),
        tone: "destructive",
        onConfirm: () => executeRemoveCapability(row, action),
      });
    },
    [executeRemoveCapability, groupId, t],
  );

  const executeBlockCapability = useCallback(
    async (row: CapabilityOverviewItem, nextBlocked: boolean) => {
      const capId = String(row.capability_id || "").trim();
      if (!groupId || !capId) return;
      setBusyKey(`block:${capId}`);
      setErr("");
      try {
        // Blocking applies everywhere; lifting a block an agent set for this Group lifts it here.
        const scope = !nextBlocked && row.blocked_group && !row.blocked_global ? "group" : "global";
        const resp = await api.setCapabilityBlocked(
          capId,
          nextBlocked,
          "capability center policy update",
          groupId,
          scope,
        );
        if (!resp.ok) {
          setErr(resp.error?.message || t("capabilityCenter.block.failed"));
          return;
        }
        rememberStickyItem({
          ...row,
          blocked_global: nextBlocked,
          policy_level: nextBlocked ? "blocked" : "actionable",
          qualification_status: nextBlocked
            ? "blocked"
            : String(row.qualification_status || "").toLowerCase() === "blocked"
              ? "ready"
              : row.qualification_status,
        });
        await refresh();
        publishCapabilityChanged(groupId);
      } finally {
        setBusyKey("");
      }
    },
    [groupId, refresh, rememberStickyItem, setErr, t],
  );

  const toggleBlockCapability = useCallback(
    (row: CapabilityOverviewItem) => {
      const capId = String(row.capability_id || "").trim();
      if (!groupId || !capId) return;
      const blocked = capabilityCenterIsBlocked(row);
      setConfirmDialog({
        title: t(
          blocked
            ? "capabilityCenter.block.confirmUnblockTitle"
            : "capabilityCenter.block.confirmBlockTitle",
          { name: capabilityCenterDisplayName(row), id: capId },
        ),
        description: t(
          blocked
            ? "capabilityCenter.block.confirmUnblockDescription"
            : "capabilityCenter.block.confirmBlockDescription",
          { name: capabilityCenterDisplayName(row), id: capId },
        ),
        confirmLabel: t(
          blocked ? "capabilityCenter.block.unblock" : "capabilityCenter.block.block",
        ),
        tone: blocked ? "default" : "destructive",
        onConfirm: () => executeBlockCapability(row, !blocked),
      });
    },
    [executeBlockCapability, groupId, t],
  );

  const executeEnableCapability = useCallback(
    async (row: CapabilityOverviewItem) => {
      const capId = String(row.capability_id || "").trim();
      if (!groupId || !capId) return;
      setBusyKey(`enable:${capId}`);
      setErr("");
      try {
        const resp = await api.enableGroupCapability(groupId, capId, {
          enabled: true,
          scope: "group",
          actorId: "user",
          reason: "capability center enable",
        });
        if (!resp.ok) {
          setErr(resp.error?.message || t("capabilityCenter.enable.failed"));
          return;
        }
        rememberStickyItem(row);
        await refresh();
        publishCapabilityChanged(groupId);
      } finally {
        setBusyKey("");
      }
    },
    [groupId, refresh, rememberStickyItem, setErr, t],
  );

  // "Enabled" here also counts the user's own actor and session use, so Disable turns off
  // whichever of them the usage reports, then shows the state it reads back.
  const executeDisableCapability = useCallback(
    async (row: CapabilityOverviewItem) => {
      const capId = String(row.capability_id || "").trim();
      if (!groupId || !capId) return;
      setBusyKey(`enable:${capId}`);
      setErr("");
      let failure = "";
      let wrote = false;
      try {
        const { result, confirmed } = await capabilityCenterTurnOff({
          readUsage: async () => {
            const resp = await api.fetchGroupCapabilityState(groupId, "user", {
              capabilityId: capId,
              noCache: true,
            });
            if (resp.ok) return resp.result?.capability_usage || null;
            failure = resp.error?.message || "";
            return null;
          },
          turnOff: async (scope) => {
            wrote = true;
            const resp = await api.enableGroupCapability(groupId, capId, {
              enabled: false,
              scope,
              actorId: "user",
              reason: "capability center disable",
            });
            if (!resp.ok) failure = resp.error?.message || "";
            return resp.ok;
          },
        });
        const message = failure || t("capabilityCenter.enable.failed");
        if (result === "unread") {
          setErr(message);
          return;
        }
        if (wrote) rememberStickyItem(row);
        const nextState = await refresh();
        if (wrote) publishCapabilityChanged(groupId);
        // A read-back that failed reports its own error, and one a newer load replaced leaves
        // that load to show the state; neither is taken to mean the capability is off.
        if (result === "failed") {
          setErr(confirmed ? t("capabilityCenter.enable.partlyFailed", { message }) : message);
        } else if (nextState && capabilityCenterEnabledIds(nextState).has(capId)) {
          setErr(
            t("capabilityCenter.enable.stillEnabled", { name: capabilityCenterDisplayName(row) }),
          );
        }
      } finally {
        setBusyKey("");
      }
    },
    [groupId, refresh, rememberStickyItem, setErr, t],
  );

  const toggleEnableCapability = useCallback(
    (row: CapabilityOverviewItem) => {
      const capId = String(row.capability_id || "").trim();
      if (!groupId || !capId) return;
      const enabledIds = capabilityCenterEnabledIds(state);
      const enabled = enabledIds.has(capId);
      if (!enabled) {
        void executeEnableCapability(row);
        return;
      }
      setConfirmDialog({
        title: t("capabilityCenter.enable.confirmDisableTitle", {
          name: capabilityCenterDisplayName(row),
          id: capId,
        }),
        description: t("capabilityCenter.enable.confirmDisableDescription", {
          name: capabilityCenterDisplayName(row),
          id: capId,
        }),
        confirmLabel: t("capabilityCenter.enable.disable"),
        onConfirm: () => executeDisableCapability(row),
      });
    },
    [executeDisableCapability, executeEnableCapability, groupId, state, t],
  );

  const executeUpdateCapability = useCallback(
    async (row: CapabilityOverviewItem) => {
      const capId = String(row.capability_id || "").trim();
      const fingerprint = String(row.update?.recipe_fingerprint || "").trim();
      if (!capId || !fingerprint) return;
      setBusyKey(`update:${capId}`);
      setErr("");
      setUpdateOutcome(null);
      try {
        const resp = await api.updateCapabilityInstallation(capId, fingerprint, groupId);
        if (!resp.ok) {
          const code = String(resp.error?.code || "");
          setErr(
            UPDATE_ERROR_CODES.has(code)
              ? t(`capabilityCenter.update.error.${code}`)
              : resp.error?.message || t("capabilityCenter.update.failed"),
          );
          return;
        }
        setUpdateOutcome({ capabilityId: capId, state: String(resp.result?.state || "") });
        await refresh();
        if (groupId) publishCapabilityChanged(groupId);
      } finally {
        setBusyKey("");
      }
    },
    [groupId, refresh, setErr, t],
  );

  const confirmUpdateCapability = useCallback(
    (row: CapabilityOverviewItem) => {
      const capId = String(row.capability_id || "").trim();
      if (!capId || !row.update?.available) return;
      setConfirmDialog({
        title: t("capabilityCenter.update.confirmTitle", {
          name: capabilityCenterDisplayName(row),
        }),
        // A token MCP is switched without a check; each member checks it on next use.
        description: t(
          row.support?.auth === "bearer"
            ? "capabilityCenter.update.confirmDescriptionBearer"
            : "capabilityCenter.update.confirmDescription",
          { groups: Number(row.update.enabled_group_count || 0) },
        ),
        confirmLabel: t(
          row.update.reason === "recipe_changed"
            ? "capabilityCenter.update.update"
            : "capabilityCenter.update.verify",
        ),
        onConfirm: () => executeUpdateCapability(row),
      });
    },
    [executeUpdateCapability, t],
  );

  const deleteSource = useCallback(
    async (source: CapabilitySourceState) => {
      const sourceId = String(source.source_id || "").trim();
      if (!groupId || !sourceId || capabilityCenterSourceRemovalAction(source) === "none") return;
      setBusyKey(`source-delete:${sourceId}`);
      setErr("");
      try {
        const resp = await api.deleteCapabilitySource(groupId, sourceId, {
          actorId: "user",
          reason: "capability center source delete",
        });
        if (!resp.ok) {
          setErr(resp.error?.message || t("capabilityCenter.sources.deleteFailed"));
          return;
        }
        await refresh();
        publishCapabilityChanged(groupId);
      } finally {
        setBusyKey("");
      }
    },
    [groupId, refresh, setErr, t],
  );

  const confirmDeleteSource = useCallback(
    (source: CapabilitySourceState) => {
      const sourceId = String(source.source_id || "").trim();
      if (!groupId || !sourceId || capabilityCenterSourceRemovalAction(source) === "none") return;
      setConfirmDialog({
        title: t("capabilityCenter.sources.deleteConfirmTitle", {
          id: sourceId,
          count: Number(source.record_count || 0),
        }),
        description: t("capabilityCenter.sources.deleteConfirmDescription", {
          id: sourceId,
          count: Number(source.record_count || 0),
        }),
        confirmLabel: t("capabilityCenter.sources.delete"),
        tone: "destructive",
        onConfirm: () => deleteSource(source),
      });
    },
    [deleteSource, groupId, t],
  );

  if (!isOpen) return null;

  return (
    <div
      ref={modalRef}
      className={capabilityCenterRootClass()}
      role="dialog"
      aria-modal
      aria-label={t("capabilityCenter.title")}
      data-capability-center
    >
      <div className={capabilityCenterFrameClass()}>
        <aside className="hidden w-[248px] shrink-0 border-r border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] md:flex md:flex-col">
          <div className="border-b border-[var(--glass-border-subtle)] px-4 py-4">
            <div className="text-sm font-semibold">{t("capabilityCenter.title")}</div>
            <div className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("capabilityCenter.subtitle")}
            </div>
          </div>
          <nav className="flex-1 space-y-1 overflow-y-auto px-2 py-3" data-capability-nav>
            {localizedSections.map((item) => {
              const Icon = item.icon;
              const active = section === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  className={`flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left transition-colors ${
                    active
                      ? "bg-[var(--glass-bg-active)] text-[var(--color-text-primary)]"
                      : "text-[var(--color-text-secondary)] hover:bg-[var(--glass-bg-hover)] hover:text-[var(--color-text-primary)]"
                  }`}
                  aria-current={active ? "page" : undefined}
                  onClick={() => setSection(item.id)}
                >
                  <LcdIcon icon={Icon} glyph={item.glyph} size={17} aria-hidden="true" />
                  <span className="min-w-0">
                    <span className="block text-sm font-medium">{item.label}</span>
                    <span className="block truncate text-[11px] text-[var(--color-text-muted)]">
                      {item.hint}
                    </span>
                  </span>
                </button>
              );
            })}
          </nav>
          <div className="border-t border-[var(--glass-border-subtle)] px-4 py-3 text-xs text-[var(--color-text-muted)]">
            {section === "recommended"
              ? t("capabilityCenter.recommended.summary", {
                  total: recommendationCount(recommendedRows),
                  domains: recommendedRows.length,
                })
              : t("capabilityCenter.sidebarSummary", {
                  capabilities: totalCount || stats.total,
                  sources: Object.keys(sources || {}).length,
                })}
          </div>
        </aside>

        <main className="flex min-h-0 min-w-0 flex-1 flex-col">
          <header className="flex min-h-[56px] items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-3 py-2 sm:px-4 sm:py-3 lg:px-6">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <LcdIcon icon={BookOpen} glyph="book-open" size={18} aria-hidden="true" />
                <h2 className="min-w-0 truncate text-base font-semibold sm:text-lg">
                  {t("capabilityCenter.title")}
                </h2>
              </div>
              <p className="mt-0.5 hidden truncate text-xs text-[var(--color-text-muted)] sm:block">
                {groupId
                  ? t("capabilityCenter.scopeGroup", { name: groupTitle || groupId })
                  : t("capabilityCenter.headerHint")}
              </p>
            </div>
            <div className="flex shrink-0 items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="h-10 w-10 bg-[var(--glass-panel-bg)] sm:w-auto sm:px-3"
                disabled={loading}
                onClick={() => {
                  setStickyItems([]);
                  setRemovedCapabilityIds(new Set());
                  void refresh();
                }}
              >
                <LcdIcon icon={RefreshCcw} glyph="reload" size={15} aria-hidden="true" />
                <span className="sr-only sm:not-sr-only">{t("capabilityCenter.refresh")}</span>
              </Button>
              {onClose ? (
                <TooltipIconButton label={t("capabilityCenter.close")}>
                  {(referenceProps, setReference) => (
                    <span ref={setReference} {...referenceProps}>
                      <Button
                        type="button"
                        variant="outline"
                        size="icon"
                        className="h-10 w-10 bg-[var(--glass-panel-bg)]"
                        aria-label={t("capabilityCenter.close")}
                        onClick={onClose}
                      >
                        <LcdIcon icon={X} glyph="close" size={17} aria-hidden="true" />
                      </Button>
                    </span>
                  )}
                </TooltipIconButton>
              ) : null}
            </div>
          </header>

          <nav
            className="flex gap-2 overflow-x-auto border-b border-[var(--glass-border-subtle)] px-3 py-2 md:hidden [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
            aria-label={t("capabilityCenter.sectionNavigation")}
            data-capability-nav
          >
            {localizedSections.map((item) => {
              const Icon = item.icon;
              const active = section === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  className={`inline-flex min-h-[40px] shrink-0 items-center gap-2 rounded-lg border px-3 text-sm font-medium ${
                    active
                      ? "border-[rgba(59,130,246,0.35)] bg-[var(--glass-bg-active)] text-[var(--color-text-primary)]"
                      : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)]"
                  }`}
                  aria-current={active ? "page" : undefined}
                  onClick={() => setSection(item.id)}
                >
                  <LcdIcon icon={Icon} glyph={item.glyph} size={15} aria-hidden="true" />
                  <span>{item.label}</span>
                </button>
              );
            })}
          </nav>

          <div className="grid min-h-0 flex-1 grid-cols-1 lg:grid-cols-[minmax(0,1fr)_360px]">
            <section
              className={`grid min-h-0 min-w-0 overflow-hidden border-[var(--glass-border-subtle)] lg:border-r ${
                section === "recommended" && !err
                  ? "grid-rows-[minmax(0,1fr)]"
                  : "grid-rows-[auto_minmax(0,1fr)]"
              }`}
            >
              {section === "recommended" && !err ? null : (
                <div
                  className={`min-w-0 border-[var(--glass-border-subtle)] ${
                    section === "sources" && !err
                      ? "sm:border-b sm:px-4 sm:py-3 lg:px-6"
                      : "border-b px-3 py-2 sm:px-4 sm:py-3 lg:px-6"
                  }`}
                >
                  {section !== "recommended" ? (
                    <div className="hidden grid-cols-2 gap-2 sm:grid sm:grid-cols-4">
                      <Stat label={t("capabilityCenter.stats.skills")} value={stats.skills} />
                      <Stat label={t("capabilityCenter.stats.mcp")} value={stats.mcp} />
                      <Stat label={t("capabilityCenter.stats.packs")} value={stats.packs} />
                      <Stat label={t("capabilityCenter.stats.enabled")} value={stats.enabled} />
                    </div>
                  ) : null}
                  {listSection ? (
                    <>
                      <div className="flex items-center gap-2 sm:hidden">
                        <button
                          type="button"
                          className="inline-flex min-h-[36px] items-center gap-2 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 text-xs font-medium text-[var(--color-text-secondary)]"
                          aria-expanded={mobileControlsOpen}
                          onClick={() => setMobileControlsOpen((value) => !value)}
                          data-lcd-key
                        >
                          <LcdIcon
                            icon={SlidersHorizontal}
                            glyph="sliders-horizontal"
                            size={14}
                            aria-hidden="true"
                          />
                          <span>{t("capabilityCenter.stateFilter")}</span>
                        </button>
                        <span className="min-w-0 truncate text-xs text-[var(--color-text-muted)]">
                          {`${t(`capabilityCenter.stateOptions.${scopedStateFilter}`)} · ${pageRange.total || totalCount || stats.total}`}
                        </span>
                      </div>
                      <div
                        className={`${mobileControlsOpen ? "mt-2 flex" : "hidden"} flex-col gap-2 sm:mt-3 sm:flex xl:flex-row xl:items-center`}
                        data-capability-filters
                      >
                        <div className="relative min-w-0 flex-1">
                          <LcdIcon
                            icon={Search}
                            glyph="search"
                            className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-[var(--color-text-muted)]"
                            size={15}
                            aria-hidden="true"
                          />
                          <input
                            className="h-9 w-full rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] pl-9 pr-3 text-sm outline-none focus:border-[rgba(59,130,246,0.42)] sm:h-10"
                            data-lcd-field
                            value={query}
                            placeholder={t("capabilityCenter.searchPlaceholder")}
                            onChange={(event) => setQuery(event.target.value)}
                          />
                        </div>
                        <div className="flex min-w-0 items-center gap-2">
                          <SegmentedControl
                            label={t("capabilityCenter.stateFilter")}
                            value={scopedStateFilter}
                            options={localizedStateFilters}
                            onChange={(value) =>
                              setStateFilter(value as CapabilityCenterStateFilter)
                            }
                          />
                          <button
                            type="button"
                            className={`min-h-[36px] self-start rounded-lg border px-3 text-xs font-medium sm:h-9 ${
                              showSystem
                                ? "border-[rgba(59,130,246,0.35)] bg-[var(--glass-bg-active)] text-[var(--color-text-primary)]"
                                : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-bg-hover)]"
                            }`}
                            aria-pressed={showSystem}
                            onClick={() => setShowSystem((value) => !value)}
                          >
                            {t("capabilityCenter.showSystem")}
                          </button>
                        </div>
                      </div>
                    </>
                  ) : null}
                  {err ? (
                    <div
                      className="mt-2 rounded-lg border border-rose-500/25 bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300"
                      data-status-tone="error"
                    >
                      {err}
                    </div>
                  ) : null}
                </div>
              )}

              {section === "sources" ? (
                <SourcesView sources={sources} busyKey={busyKey} onDelete={confirmDeleteSource} />
              ) : section === "recommended" ? (
                <div className="grid min-h-0">
                  <div
                    className={recommendedDetailOpen ? "hidden min-h-0 lg:grid" : "grid min-h-0"}
                  >
                    <CapabilityRecommendedView
                      rows={recommendedRows}
                      groupTitle={groupId ? groupTitle || groupId : ""}
                      loading={loading}
                      selectedId={String(recommendedSelected?.capability_id || "")}
                      explicit={
                        Boolean(recommendedId) &&
                        recommendedSelected?.capability_id === recommendedId
                      }
                      onSelect={(capabilityId) => {
                        setRecommendedId(capabilityId);
                        setRecommendedDetailOpen(true);
                      }}
                    />
                  </div>
                  {recommendedDetailOpen && recommendedSelected ? (
                    <div className="min-h-0 overflow-y-auto lg:hidden" data-capability-detail-page>
                      <div className="px-3 pt-3 sm:px-4">
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          autoFocus
                          onClick={() => setRecommendedDetailOpen(false)}
                        >
                          <LcdIcon
                            icon={ChevronLeft}
                            glyph="chevron-left"
                            size={14}
                            aria-hidden="true"
                          />
                          <span>{t("capabilityCenter.recommended.backToList")}</span>
                        </Button>
                      </div>
                      <CapabilityDetails
                        layout="page"
                        item={recommendedSelected}
                        state={state}
                        hiddenIds={hiddenIds}
                        groupId={groupId}
                        domains={domains}
                        busyKey={busyKey}
                        onToggleSlash={toggleSlashVisibility}
                        onToggleBlock={toggleBlockCapability}
                        onToggleEnable={toggleEnableCapability}
                        onRemove={removeCapability}
                        onUpdate={confirmUpdateCapability}
                        updateOutcome={updateOutcome}
                      />
                    </div>
                  ) : null}
                </div>
              ) : (
                <div className="grid min-h-0 grid-rows-[minmax(0,1fr)_auto]">
                  <CapabilityTable
                    items={visibleItems}
                    selectedId={String(selected?.capability_id || "")}
                    state={state}
                    hiddenIds={hiddenIds}
                    busyKey={busyKey}
                    loading={loading}
                    onSelect={setSelectedId}
                    onToggleSlash={toggleSlashVisibility}
                    onRemove={removeCapability}
                    onToggleBlock={toggleBlockCapability}
                    onToggleEnable={toggleEnableCapability}
                  />
                  <CapabilityPaginationBar
                    loading={loading}
                    pageIndex={normalizedPage.pageIndex}
                    pageSize={normalizedPage.pageSize}
                    totalPages={normalizedPage.totalPages}
                    totalCount={pageRange.total}
                    rangeFrom={pageRange.from}
                    rangeTo={pageRange.to}
                    hasMore={clientPagination ? false : hasMore}
                    onPageIndexChange={setPageIndex}
                    onPageSizeChange={(nextPageSize) => {
                      setPageSize(nextPageSize);
                      setPageIndex(0);
                    }}
                  />
                </div>
              )}
            </section>

            {section === "sources" ? (
              <SourcesSummary sources={sources} />
            ) : (
              <CapabilityDetails
                item={section === "recommended" ? recommendedSelected : selected}
                state={state}
                hiddenIds={hiddenIds}
                groupId={groupId}
                domains={domains}
                busyKey={busyKey}
                onToggleSlash={toggleSlashVisibility}
                onToggleBlock={toggleBlockCapability}
                onToggleEnable={toggleEnableCapability}
                onRemove={removeCapability}
                onUpdate={confirmUpdateCapability}
                updateOutcome={updateOutcome}
              />
            )}
          </div>
        </main>
        <Dialog
          open={Boolean(confirmDialog)}
          onOpenChange={(open) => {
            if (!open && !busyKey) setConfirmDialog(null);
          }}
        >
          <DialogContent className="w-[min(calc(100vw-1.5rem),28rem)] p-5">
            <DialogHeader>
              <DialogTitle>{confirmDialog?.title || ""}</DialogTitle>
              <DialogDescription className="leading-6">
                {confirmDialog?.description || ""}
              </DialogDescription>
            </DialogHeader>
            <DialogFooter className="mt-5">
              <Button
                type="button"
                variant="outline"
                onClick={() => setConfirmDialog(null)}
                disabled={Boolean(busyKey)}
              >
                {t("capabilityCenter.confirm.cancel")}
              </Button>
              <Button
                type="button"
                variant={confirmDialog?.tone === "destructive" ? "destructive" : "default"}
                disabled={Boolean(busyKey)}
                onClick={() => {
                  const pending = confirmDialog;
                  if (!pending) return;
                  void Promise.resolve(pending.onConfirm()).then(() => setConfirmDialog(null));
                }}
              >
                {confirmDialog?.confirmLabel || t("capabilityCenter.confirm.confirm")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="min-w-0 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 py-2">
      <div className="truncate text-[11px] text-[var(--color-text-muted)]">{label}</div>
      <div className="text-lg font-semibold tabular-nums sm:text-xl">{value}</div>
    </div>
  );
}

function SegmentedControl(props: {
  label: string;
  value: string;
  options: Array<{ id: string; label: string }>;
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <div className="min-w-0 flex-1 sm:flex-none" aria-label={props.label}>
      <div
        className={`flex max-w-full items-center gap-1 overflow-x-auto rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden ${props.disabled ? "opacity-50" : ""}`}
      >
        <LcdIcon
          icon={SlidersHorizontal}
          glyph="sliders-horizontal"
          size={14}
          className="ml-1 shrink-0 text-[var(--color-text-muted)]"
          aria-hidden="true"
        />
        {props.options.map((option) => (
          <button
            key={option.id}
            type="button"
            className={`min-h-[34px] shrink-0 rounded-md px-3 text-xs font-medium sm:min-h-[36px] ${props.value === option.id ? "bg-[var(--glass-bg-active)] text-[var(--color-text-primary)]" : "text-[var(--color-text-secondary)] hover:bg-[var(--glass-bg-hover)]"}`}
            disabled={props.disabled}
            aria-pressed={props.value === option.id}
            onClick={() => props.onChange(option.id)}
          >
            {option.label}
          </button>
        ))}
      </div>
    </div>
  );
}

function CapabilityTable(props: {
  items: CapabilityOverviewItem[];
  selectedId: string;
  state: CapabilityStateResult | null;
  hiddenIds: string[];
  busyKey: string;
  loading: boolean;
  onSelect: (capabilityId: string) => void;
  onToggleSlash: (row: CapabilityOverviewItem) => void;
  onRemove: (
    row: CapabilityOverviewItem,
    action: ReturnType<typeof capabilityCenterRemovalAction>,
  ) => void;
  onToggleBlock: (row: CapabilityOverviewItem) => void;
  onToggleEnable: (row: CapabilityOverviewItem) => void;
}) {
  const { t } = useTranslation("settings");
  const enabledIds = useMemo(
    () =>
      new Set([
        ...(props.state?.enabled_capabilities || []),
        ...(props.state?.enabled || []).map((item) => item.capability_id),
      ]),
    [props.state],
  );

  if (props.loading && props.items.length === 0) {
    return (
      <div className="p-6 text-sm text-[var(--color-text-muted)]">
        {t("capabilityCenter.loading")}
      </div>
    );
  }

  return (
    <div className="h-full min-h-0 overflow-auto scrollbar-subtle">
      <div className="hidden md:block">
        <div className="sticky top-0 z-[1] grid min-w-[812px] grid-cols-[minmax(176px,1.8fr)_88px_150px_130px_170px] gap-3 border-b border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-4 py-2 text-[11px] font-semibold uppercase tracking-[0.06em] text-[var(--color-text-muted)] lg:px-6">
          <div>{t("capabilityCenter.table.capability")}</div>
          <div>{t("capabilityCenter.table.type")}</div>
          <div>{t("capabilityCenter.table.state")}</div>
          <div>{t("capabilityCenter.table.source")}</div>
          <div>{t("capabilityCenter.table.actions")}</div>
        </div>
        <div className="min-w-[812px] divide-y divide-[var(--glass-border-subtle)]">
          {props.items.map((row) => {
            const capId = String(row.capability_id || "").trim();
            const selected = capId === props.selectedId;
            const type = capabilityCenterType(row);
            const enabled = enabledIds.has(capId);
            const hidden = isCapabilityHiddenFromSlashCommands(capId, props.hiddenIds);
            const canShowSlashToggle = enabled && canManageSlashCommandVisibility(row);
            const blocked = capabilityCenterIsBlocked(row);
            const needsSetup = capabilityCenterNeedsSetup(row);
            const recommended = isReleaseRecommendation(row);
            const removalAction = capabilityCenterRemovalAction(row, { enabled });
            return (
              <div
                key={capId}
                role="button"
                tabIndex={0}
                className={`grid w-full grid-cols-[minmax(176px,1.8fr)_88px_150px_130px_170px] items-center gap-3 px-4 py-2.5 text-left text-sm lg:px-6 ${
                  selected ? "bg-[var(--glass-bg-active)]" : "hover:bg-[var(--glass-bg-hover)]"
                }`}
                aria-current={selected ? "true" : undefined}
                onClick={() => props.onSelect(capId)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    props.onSelect(capId);
                  }
                }}
              >
                <div className="min-w-0">
                  <div className="truncate font-medium">{capabilityCenterDisplayName(row)}</div>
                  <div className="truncate text-xs text-[var(--color-text-muted)]">{capId}</div>
                </div>
                <div>
                  <MiniBadge>{capabilityCenterTypeLabel(type)}</MiniBadge>
                </div>
                <div className="flex flex-wrap items-center gap-1">
                  <UpdateMark row={row} />
                  {enabled && !recommended ? (
                    <MiniBadge kind="good">{t("capabilityCenter.status.enabled")}</MiniBadge>
                  ) : null}
                  {row.legacy_search_unverified ? (
                    <MiniBadge
                      kind="warn"
                      title={t("capabilityCenter.status.legacySearchUnverifiedHint")}
                    >
                      {t("capabilityCenter.status.legacySearchUnverified")}
                    </MiniBadge>
                  ) : null}
                  {recommended ? (
                    // The recommended table's own summary of the members, so no list says "Ready"
                    // for something no member can use.
                    <RecommendationDotIcon dot={recommendationDot(row)} />
                  ) : blocked ? (
                    <MiniBadge kind="danger">{t("capabilityCenter.status.blocked")}</MiniBadge>
                  ) : needsSetup ? (
                    <MiniBadge kind="warn">{t("capabilityCenter.status.setup")}</MiniBadge>
                  ) : !row.legacy_search_unverified ? (
                    <MiniBadge>{t("capabilityCenter.status.ready")}</MiniBadge>
                  ) : null}
                </div>
                <div className="truncate text-xs text-[var(--color-text-secondary)]">
                  {recommended ? <RecommendedMark /> : row.source_id || "-"}
                </div>
                <CapabilityRowActions
                  row={row}
                  enabled={enabled}
                  hidden={hidden}
                  blocked={blocked}
                  canEnable={!recommended || recommendationCanEnableForGroup(row)}
                  canShowSlashToggle={canShowSlashToggle}
                  removalAction={removalAction}
                  busyKey={props.busyKey}
                  onToggleEnable={props.onToggleEnable}
                  onToggleSlash={props.onToggleSlash}
                  onToggleBlock={props.onToggleBlock}
                  onRemove={props.onRemove}
                />
              </div>
            );
          })}
          {props.items.length === 0 ? (
            <div className="px-6 py-10 text-sm text-[var(--color-text-muted)]">
              {t("capabilityCenter.noCapabilities")}
            </div>
          ) : null}
        </div>
      </div>
      <div className="grid gap-2 p-2 md:hidden">
        {props.items.map((row) => {
          const capId = String(row.capability_id || "").trim();
          const selected = capId === props.selectedId;
          const type = capabilityCenterType(row);
          const enabled = enabledIds.has(capId);
          const hidden = isCapabilityHiddenFromSlashCommands(capId, props.hiddenIds);
          const canShowSlashToggle = enabled && canManageSlashCommandVisibility(row);
          const blocked = capabilityCenterIsBlocked(row);
          const needsSetup = capabilityCenterNeedsSetup(row);
          const recommended = isReleaseRecommendation(row);
          const removalAction = capabilityCenterRemovalAction(row, { enabled });
          return (
            <div
              key={capId}
              role="button"
              tabIndex={0}
              className={`rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-3 text-left text-sm ${
                selected ? "ring-1 ring-[rgba(59,130,246,0.42)]" : ""
              }`}
              aria-current={selected ? "true" : undefined}
              onClick={() => props.onSelect(capId)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  props.onSelect(capId);
                }
              }}
            >
              <div className="flex min-w-0 items-start justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="break-words font-medium [overflow-wrap:anywhere]">
                    {capabilityCenterDisplayName(row)}
                  </div>
                  <div className="mt-0.5 break-words text-xs text-[var(--color-text-muted)] [overflow-wrap:anywhere]">
                    {capId}
                  </div>
                </div>
                <div className="flex shrink-0 flex-wrap justify-end gap-1">
                  {recommended ? <RecommendedMark /> : null}
                  <MiniBadge>{capabilityCenterTypeLabel(type)}</MiniBadge>
                </div>
              </div>
              <div className="mt-2 flex flex-wrap items-center gap-1.5">
                <UpdateMark row={row} />
                {enabled && !recommended ? (
                  <MiniBadge kind="good">{t("capabilityCenter.status.enabled")}</MiniBadge>
                ) : null}
                {row.legacy_search_unverified ? (
                  <MiniBadge
                    kind="warn"
                    title={t("capabilityCenter.status.legacySearchUnverifiedHint")}
                  >
                    {t("capabilityCenter.status.legacySearchUnverified")}
                  </MiniBadge>
                ) : null}
                {recommended ? (
                  <RecommendationDotIcon dot={recommendationDot(row)} />
                ) : blocked ? (
                  <MiniBadge kind="danger">{t("capabilityCenter.status.blocked")}</MiniBadge>
                ) : needsSetup ? (
                  <MiniBadge kind="warn">{t("capabilityCenter.status.setup")}</MiniBadge>
                ) : !row.legacy_search_unverified ? (
                  <MiniBadge>{t("capabilityCenter.status.ready")}</MiniBadge>
                ) : null}
                {row.source_id && !recommended ? <MiniBadge>{row.source_id}</MiniBadge> : null}
              </div>
              {row.legacy_search_unverified && selected ? (
                <p className="mt-2 text-xs text-[var(--color-text-secondary)]">
                  {t("capabilityCenter.status.legacySearchUnverifiedHint")}
                </p>
              ) : null}
              <CapabilityRowActions
                row={row}
                enabled={enabled}
                hidden={hidden}
                blocked={blocked}
                canEnable={!recommended || recommendationCanEnableForGroup(row)}
                canShowSlashToggle={canShowSlashToggle}
                removalAction={removalAction}
                busyKey={props.busyKey}
                onToggleEnable={props.onToggleEnable}
                onToggleSlash={props.onToggleSlash}
                onToggleBlock={props.onToggleBlock}
                onRemove={props.onRemove}
                compact
              />
            </div>
          );
        })}
        {props.items.length === 0 ? (
          <div className="px-6 py-10 text-sm text-[var(--color-text-muted)]">
            {t("capabilityCenter.noCapabilities")}
          </div>
        ) : null}
      </div>
    </div>
  );
}

/** Where a release recommendation comes from, said instead of its catalog id. */
function RecommendedMark() {
  const { t } = useTranslation("settings");
  return (
    <MiniBadge title={t("capabilityCenter.recommended.badgeHint")}>
      {t("capabilityCenter.recommended.badge")}
    </MiniBadge>
  );
}

/** A recipe that changed since it was installed. */
function UpdateMark({ row }: { row: CapabilityOverviewItem }) {
  const { t } = useTranslation("settings");
  return row.update?.available && row.update.reason === "recipe_changed" ? (
    <MiniBadge kind="warn">{t("capabilityCenter.update.badge")}</MiniBadge>
  ) : null;
}

function CapabilityRowActions(props: {
  row: CapabilityOverviewItem;
  enabled: boolean;
  hidden: boolean;
  blocked: boolean;
  /** False hides enabling when no member of the Group could use it; disabling stays. */
  canEnable?: boolean;
  canShowSlashToggle: boolean;
  removalAction: ReturnType<typeof capabilityCenterRemovalAction>;
  busyKey: string;
  compact?: boolean;
  onToggleSlash: (row: CapabilityOverviewItem) => void;
  onRemove: (
    row: CapabilityOverviewItem,
    action: ReturnType<typeof capabilityCenterRemovalAction>,
  ) => void;
  onToggleBlock: (row: CapabilityOverviewItem) => void;
  onToggleEnable: (row: CapabilityOverviewItem) => void;
}) {
  const { t } = useTranslation("settings");
  const capId = String(props.row.capability_id || "").trim();
  return (
    <div className={cn("flex gap-1.5", props.compact ? "mt-3 justify-start" : "justify-end")}>
      {props.enabled || props.canEnable !== false ? (
        <EnableCapabilityButton
          enabled={props.enabled}
          busy={props.busyKey === `enable:${capId}`}
          compact={props.compact}
          onClick={(event) => {
            event.stopPropagation();
            props.onToggleEnable(props.row);
          }}
        />
      ) : null}
      {props.canShowSlashToggle ? (
        <IconToggle
          hidden={props.hidden}
          busy={props.busyKey === `slash:${capId}`}
          compact={props.compact}
          onClick={(event) => {
            event.stopPropagation();
            props.onToggleSlash(props.row);
          }}
        />
      ) : null}
      <BlockCapabilityButton
        blocked={props.blocked}
        busy={props.busyKey === `block:${capId}`}
        compact={props.compact}
        onClick={(event) => {
          event.stopPropagation();
          props.onToggleBlock(props.row);
        }}
      />
      {props.removalAction !== "none" ? (
        <RemoveCapabilityButton
          action={props.removalAction}
          busy={props.busyKey === `remove:${capId}`}
          compact={props.compact}
          onClick={(event) => {
            event.stopPropagation();
            props.onRemove(props.row, props.removalAction);
          }}
        />
      ) : null}
      {!props.canShowSlashToggle && props.removalAction === "none" ? (
        <span className="self-center text-xs text-[var(--color-text-muted)]">
          {t("capabilityCenter.emptyDash")}
        </span>
      ) : null}
    </div>
  );
}

function CapabilityPaginationBar(props: {
  loading: boolean;
  pageIndex: number;
  pageSize: number;
  totalPages: number;
  totalCount: number;
  rangeFrom: number;
  rangeTo: number;
  hasMore: boolean;
  onPageIndexChange: (pageIndex: number) => void;
  onPageSizeChange: (pageSize: number) => void;
}) {
  const { t } = useTranslation("settings");
  const canPrev = props.pageIndex > 0 && !props.loading;
  const canNext = props.pageIndex + 1 < props.totalPages && !props.loading;
  return (
    <div className="grid min-h-[48px] gap-2 border-t border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 py-2 text-xs text-[var(--color-text-secondary)] sm:flex sm:flex-wrap sm:items-center sm:justify-between lg:px-6">
      <div className="flex min-w-0 items-center gap-2">
        <span className="truncate">
          {props.totalCount > 0
            ? t("capabilityCenter.pagination.showing", {
                from: props.rangeFrom,
                to: props.rangeTo,
                total: props.totalCount,
              })
            : t("capabilityCenter.pagination.noResults")}
        </span>
        {props.hasMore ? (
          <MiniBadge>{t("capabilityCenter.pagination.moreAvailable")}</MiniBadge>
        ) : null}
      </div>
      <div className="grid gap-2 sm:flex sm:items-center">
        <label className="hidden items-center gap-2 sm:flex">
          <span>{t("capabilityCenter.pagination.rows")}</span>
          <select
            className="h-9 rounded-md border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-2 text-xs text-[var(--color-text-primary)] sm:h-8"
            data-lcd-field
            value={props.pageSize}
            disabled={props.loading}
            onChange={(event) => props.onPageSizeChange(Number(event.target.value))}
          >
            {CAPABILITY_CENTER_PAGE_SIZE_OPTIONS.map((size) => (
              <option key={size} value={size}>
                {size}
              </option>
            ))}
          </select>
        </label>
        <div className="grid grid-cols-[minmax(0,1fr)_64px_minmax(0,1fr)] items-center gap-2 sm:flex">
          <button
            type="button"
            className="inline-flex min-h-[36px] items-center justify-center rounded-md border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 text-xs hover:bg-[var(--glass-bg-hover)] disabled:cursor-not-allowed disabled:opacity-50 sm:h-8 sm:min-h-0"
            disabled={!canPrev}
            onClick={() => props.onPageIndexChange(Math.max(0, props.pageIndex - 1))}
            data-lcd-key
          >
            {t("capabilityCenter.pagination.prev")}
          </button>
          <span className="text-center tabular-nums sm:min-w-[72px]">
            {props.pageIndex + 1} / {props.totalPages}
          </span>
          <button
            type="button"
            className="inline-flex min-h-[36px] items-center justify-center rounded-md border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 text-xs hover:bg-[var(--glass-bg-hover)] disabled:cursor-not-allowed disabled:opacity-50 sm:h-8 sm:min-h-0"
            disabled={!canNext}
            onClick={() =>
              props.onPageIndexChange(Math.min(props.totalPages - 1, props.pageIndex + 1))
            }
            data-lcd-key
          >
            {t("capabilityCenter.pagination.next")}
          </button>
        </div>
      </div>
    </div>
  );
}

function EnableCapabilityButton({
  enabled,
  busy,
  compact,
  onClick,
}: {
  enabled: boolean;
  busy: boolean;
  compact?: boolean;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
}) {
  const { t } = useTranslation("settings");
  const Icon = enabled ? PowerOff : Power;
  const label = enabled
    ? t("capabilityCenter.enable.disable")
    : t("capabilityCenter.enable.enable");
  const tooltip = enabled
    ? t("capabilityCenter.enable.disableTitle")
    : t("capabilityCenter.enable.enableTitle");
  return (
    <TooltipIconButton label={tooltip}>
      {(referenceProps, setReference) => (
        <span ref={setReference} {...referenceProps}>
          <button
            type="button"
            className={cn(
              "inline-flex items-center justify-center rounded-md border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-primary)] hover:bg-[var(--glass-bg-hover)] disabled:opacity-60",
              compact ? "h-10 w-10" : "h-8 w-8",
              busy ? "cursor-not-allowed opacity-60" : "",
            )}
            disabled={busy}
            aria-label={label}
            onClick={onClick}
            data-lcd-key
          >
            <LcdIcon
              icon={Icon}
              glyph={enabled ? "power-off" : "power"}
              size={14}
              aria-hidden="true"
            />
          </button>
        </span>
      )}
    </TooltipIconButton>
  );
}

function IconToggle({
  hidden,
  busy,
  compact,
  onClick,
}: {
  hidden: boolean;
  busy: boolean;
  compact?: boolean;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
}) {
  const { t } = useTranslation("settings");
  const Icon = hidden ? EyeOff : Eye;
  const label = hidden ? t("capabilityCenter.slashHidden") : t("capabilityCenter.slashVisible");
  const tooltip = hidden
    ? t("capabilityCenter.showInSlashCommands")
    : t("capabilityCenter.hideFromSlashCommands");
  return (
    <TooltipIconButton label={tooltip}>
      {(referenceProps, setReference) => (
        <span ref={setReference} {...referenceProps}>
          <button
            type="button"
            className={`inline-flex items-center justify-center rounded-lg border text-xs ${compact ? "h-10 w-10" : "h-8 w-8"} ${
              hidden
                ? "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300"
                : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-bg-hover)]"
            } ${busy ? "cursor-not-allowed opacity-60" : ""}`}
            disabled={busy}
            aria-label={label}
            onClick={onClick}
            data-lcd-key
            data-status-tone={hidden ? "neutral" : undefined}
          >
            <LcdIcon icon={Icon} glyph={hidden ? "eye-off" : "eye"} size={14} aria-hidden="true" />
          </button>
        </span>
      )}
    </TooltipIconButton>
  );
}

function BlockCapabilityButton({
  blocked,
  busy,
  compact,
  onClick,
}: {
  blocked: boolean;
  busy: boolean;
  compact?: boolean;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
}) {
  const { t } = useTranslation("settings");
  const label = blocked ? t("capabilityCenter.block.unblock") : t("capabilityCenter.block.block");
  const tooltip = blocked
    ? t("capabilityCenter.block.unblockTitle")
    : t("capabilityCenter.block.blockTitle");
  return (
    <TooltipIconButton label={tooltip}>
      {(referenceProps, setReference) => (
        <span ref={setReference} {...referenceProps}>
          <button
            type="button"
            className={`inline-flex items-center justify-center rounded-lg border text-xs ${compact ? "h-10 w-10" : "h-8 w-8"} ${
              blocked
                ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                : "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300"
            } ${busy ? "cursor-not-allowed opacity-60" : ""}`}
            disabled={busy}
            aria-label={label}
            onClick={onClick}
            data-status-tone={blocked ? "ok" : "warning"}
          >
            <LcdIcon icon={Shield} glyph="shield" size={15} aria-hidden="true" />
          </button>
        </span>
      )}
    </TooltipIconButton>
  );
}

function RemoveCapabilityButton({
  action,
  busy,
  compact,
  onClick,
}: {
  action: Exclude<ReturnType<typeof capabilityCenterRemovalAction>, "none">;
  busy: boolean;
  compact?: boolean;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
}) {
  const { t } = useTranslation("settings");
  const label = busy
    ? t("capabilityCenter.remove.busy")
    : t(`capabilityCenter.remove.label.${action}`);
  const tooltip = t(`capabilityCenter.remove.title.${action}`);
  return (
    <TooltipIconButton label={tooltip}>
      {(referenceProps, setReference) => (
        <span ref={setReference} {...referenceProps}>
          <button
            type="button"
            className={`inline-flex items-center justify-center rounded-lg border border-rose-500/25 bg-rose-500/10 text-xs text-rose-700 hover:bg-rose-500/15 disabled:cursor-not-allowed disabled:opacity-60 dark:text-rose-300 ${compact ? "h-10 w-10" : "h-8 w-8"} ${busy ? "cursor-not-allowed opacity-60" : ""}`}
            disabled={busy}
            aria-label={label}
            onClick={onClick}
            data-status-tone="error"
          >
            <LcdIcon icon={Trash2} glyph="trash" size={14} aria-hidden="true" />
          </button>
        </span>
      )}
    </TooltipIconButton>
  );
}

function CapabilityDetails(props: {
  item: CapabilityOverviewItem | null;
  state: CapabilityStateResult | null;
  hiddenIds: string[];
  groupId: string;
  domains: CapabilityDomain[];
  busyKey: string;
  /** "page" fills a narrow screen in place of the list; the default is the side panel. */
  layout?: "aside" | "page";
  onToggleSlash: (row: CapabilityOverviewItem) => void;
  onToggleBlock: (row: CapabilityOverviewItem) => void;
  onToggleEnable: (row: CapabilityOverviewItem) => void;
  onRemove: (
    row: CapabilityOverviewItem,
    action: Exclude<ReturnType<typeof capabilityCenterRemovalAction>, "none">,
  ) => void;
  onUpdate: (row: CapabilityOverviewItem) => void;
  updateOutcome: UpdateOutcome | null;
}) {
  const { t } = useTranslation("settings");
  // In place of the list on a narrow screen it is the page's main content, not a side panel.
  const Frame = props.layout === "page" ? "section" : "aside";
  const frame =
    props.layout === "page"
      ? "min-w-0 p-3 sm:p-4"
      : "hidden min-h-0 min-w-0 overflow-x-hidden overflow-y-auto bg-[var(--glass-panel-bg)] p-5 lg:block";
  if (!props.item) {
    return (
      <Frame className={cn(frame, "text-sm text-[var(--color-text-muted)]")}>
        {t("capabilityCenter.details.selectCapability")}
      </Frame>
    );
  }
  const row = props.item;
  const capId = String(row.capability_id || "").trim();
  const type = capabilityCenterType(row);
  const recommended = isReleaseRecommendation(row);
  const enabledIds = new Set([
    ...(props.state?.enabled_capabilities || []),
    ...(props.state?.enabled || []).map((item) => item.capability_id),
  ]);
  const enabled = enabledIds.has(capId);
  const hidden = isCapabilityHiddenFromSlashCommands(capId, props.hiddenIds);
  const canShowSlashToggle = enabled && canManageSlashCommandVisibility(row);
  const blocked = capabilityCenterIsBlocked(row);
  const needsSetup = capabilityCenterNeedsSetup(row);
  const preview = row.readiness_preview;
  const removalAction = capabilityCenterRemovalAction(row, { enabled });
  const binding = recommendationBinding(row);

  return (
    <Frame className={frame}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-xs uppercase tracking-[0.08em] text-[var(--color-text-muted)]">
            {recommended
              ? t("capabilityCenter.recommended.kicker", { type: capabilityCenterTypeLabel(type) })
              : capabilityCenterTypeLabel(type)}
          </div>
          <h3 className="mt-1 break-words text-lg font-semibold [overflow-wrap:anywhere]">
            {capabilityCenterDisplayName(row)}
          </h3>
          <p className="mt-1 break-all text-xs text-[var(--color-text-muted)]">{capId}</p>
        </div>
        <div className="flex shrink-0 flex-wrap justify-end gap-1">
          {recommended ? (
            blocked ? (
              <MiniBadge kind="danger">{t("capabilityCenter.status.blocked")}</MiniBadge>
            ) : binding !== "none" ? (
              // Nothing enabled needs no word: the enable control below already says so.
              <MiniBadge kind={binding === "group" ? "good" : "neutral"}>
                {t(`capabilityCenter.recommended.binding.${binding}`)}
              </MiniBadge>
            ) : null
          ) : blocked ? (
            <MiniBadge kind="danger">{t("capabilityCenter.status.blocked")}</MiniBadge>
          ) : needsSetup ? (
            <MiniBadge kind="warn">{t("capabilityCenter.status.needsSetup")}</MiniBadge>
          ) : enabled ? (
            <MiniBadge kind="good">{t("capabilityCenter.status.enabled")}</MiniBadge>
          ) : !row.legacy_search_unverified ? (
            <MiniBadge>{t("capabilityCenter.status.ready")}</MiniBadge>
          ) : null}
          {row.legacy_search_unverified ? (
            <MiniBadge kind="warn" title={t("capabilityCenter.status.legacySearchUnverifiedHint")}>
              {t("capabilityCenter.status.legacySearchUnverified")}
            </MiniBadge>
          ) : null}
        </div>
      </div>

      <CapabilityControlsPanel
        row={row}
        enabled={enabled}
        hidden={hidden}
        blocked={blocked}
        canEnable={!recommended || recommendationCanEnableForGroup(row)}
        blockScope={row.blocked_group && !row.blocked_global ? "group" : "global"}
        canShowSlashToggle={canShowSlashToggle}
        removalAction={removalAction}
        busyKey={props.busyKey}
        groupId={props.groupId}
        onToggleEnable={props.onToggleEnable}
        onToggleSlash={props.onToggleSlash}
        onToggleBlock={props.onToggleBlock}
        onRemove={props.onRemove}
      />
      <UpdateNotice
        row={row}
        busy={props.busyKey === `update:${capId}`}
        outcome={props.updateOutcome?.capabilityId === capId ? props.updateOutcome.state : ""}
        onUpdate={props.onUpdate}
      />

      <div className="mt-5 space-y-4">
        {recommended ? (
          <RecommendationSections row={row} domains={props.domains} />
        ) : (
          <>
            {row.legacy_search_unverified ? (
              <p className="text-xs text-[var(--color-text-secondary)]">
                {t("capabilityCenter.status.legacySearchUnverifiedHint")}
              </p>
            ) : null}
            <DetailBlock title={t("capabilityCenter.details.description")}>
              {capabilityCenterDescription(row, t) || t("capabilityCenter.details.noDescription")}
            </DetailBlock>
            <DetailBlock title={t("capabilityCenter.details.source")}>
              {row.source_id || t("capabilityCenter.emptyDash")}
              {row.source_uri ? ` · ${row.source_uri}` : ""}
            </DetailBlock>
            <DetailBlock title={t("capabilityCenter.details.policy")}>
              {row.policy_level || t("capabilityCenter.emptyDash")}
              {row.legacy_search_unverified
                ? ` · ${t("capabilityCenter.status.legacySearchUnverified")}`
                : row.qualification_status
                  ? ` · ${row.qualification_status}`
                  : ""}
            </DetailBlock>
            {preview ? (
              <DetailBlock title={t("capabilityCenter.details.readiness")}>
                {[preview.preview_status, preview.next_step, preview.enable_block_reason]
                  .filter(Boolean)
                  .join(" · ") || t("capabilityCenter.emptyDash")}
              </DetailBlock>
            ) : null}
            {row.tool_names?.length ? (
              <DetailBlock title={t("capabilityCenter.details.tools")}>
                {row.tool_names.join(", ")}
              </DetailBlock>
            ) : null}
            {row.use_when?.length ? (
              <DetailList title={t("capabilityCenter.details.useWhen")} items={row.use_when} />
            ) : null}
            {row.gotchas?.length ? (
              <DetailList title={t("capabilityCenter.details.gotchas")} items={row.gotchas} />
            ) : null}
          </>
        )}
      </div>
    </Frame>
  );
}

/** A changed recipe is offered as a global update; an unrecorded one only as a check. */
function UpdateNotice(props: {
  row: CapabilityOverviewItem;
  busy: boolean;
  /** The state the last update of this capability returned, if any. */
  outcome: string;
  onUpdate: (row: CapabilityOverviewItem) => void;
}) {
  const { t } = useTranslation("settings");
  if (UPDATE_OUTCOMES.has(props.outcome)) {
    return (
      <p className="mt-3 text-xs text-[var(--color-text-muted)]" role="status">
        {t(`capabilityCenter.update.outcome.${props.outcome}`)}
      </p>
    );
  }
  const update = props.row.update;
  if (!update) return null;
  const changed = update.reason === "recipe_changed";
  if (!update.available) {
    // Only worth saying when there is something to update.
    const reason = String(update.unavailable_reason || "");
    return (changed || update.reason === "recipe_unknown") && UPDATE_UNAVAILABLE.has(reason) ? (
      <p className="mt-3 text-xs text-[var(--color-text-muted)]">
        {t(`capabilityCenter.update.unavailable.${reason}`)}
      </p>
    ) : null;
  }
  return (
    <section className="mt-3 flex flex-wrap items-center justify-between gap-2 rounded-lg border border-[var(--glass-border-subtle)] px-3 py-2">
      <span
        className={cn(
          "min-w-0 text-xs leading-5",
          changed ? "text-amber-700 dark:text-amber-300" : "text-[var(--color-text-muted)]",
        )}
        data-status-tone={changed ? "warning" : undefined}
      >
        {t(changed ? "capabilityCenter.update.changed" : "capabilityCenter.update.unknown")}
      </span>
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={props.busy}
        onClick={() => props.onUpdate(props.row)}
      >
        {t(changed ? "capabilityCenter.update.update" : "capabilityCenter.update.verify")}
      </Button>
    </section>
  );
}

function RecommendationSections(props: {
  row: CapabilityOverviewItem;
  domains: CapabilityDomain[];
}) {
  const { t } = useTranslation("settings");
  const { row } = props;
  const domainName = (domainId: string) =>
    props.domains.find((domain) => domain.domain_id === domainId)?.name || domainId;
  const authKey = recommendationAuthKey(row.support);
  const runsOnKey = recommendationRunsOnKey(row.support);
  const where = [
    authKey ? t(`capabilityCenter.recommended.auth.${authKey}`) : "",
    runsOnKey ? t(`capabilityCenter.recommended.runsOn.${runsOnKey}`) : "",
  ].filter(Boolean);
  const verification = recommendationVerification(row.verification);
  return (
    <>
      <DetailBlock title={t("capabilityCenter.recommended.sections.why")}>
        <div className="space-y-2">
          {(row.recommendation || []).map((entry) => (
            <div key={entry.domain_id}>
              <div className="text-xs text-[var(--color-text-muted)]">
                {`${domainName(entry.domain_id)} · ${t(
                  entry.role === "primary"
                    ? "capabilityCenter.recommended.primary"
                    : "capabilityCenter.recommended.alternative",
                )}`}
              </div>
              {entry.reason ? <div>{entry.reason}</div> : null}
            </div>
          ))}
        </div>
      </DetailBlock>
      {row.use_when?.length ? (
        <DetailList
          title={t("capabilityCenter.recommended.sections.useWhen")}
          items={row.use_when}
        />
      ) : null}
      {row.avoid_when?.length ? (
        <DetailList
          title={t("capabilityCenter.recommended.sections.avoidWhen")}
          items={row.avoid_when}
        />
      ) : null}
      {row.gotchas?.length ? (
        <DetailList
          title={t("capabilityCenter.recommended.sections.gotchas")}
          items={row.gotchas}
        />
      ) : null}
      {where.length || row.requirements_summary ? (
        <DetailBlock title={t("capabilityCenter.recommended.sections.requirements")}>
          {where.length ? <div>{where.join(" · ")}</div> : null}
          {row.requirements_summary ? <div>{row.requirements_summary}</div> : null}
        </DetailBlock>
      ) : null}
      <RecommendationMembers row={row} />
      <DetailBlock title={t("capabilityCenter.recommended.sections.verification")}>
        {verification ? (
          // The label's hover tip carries the check's own words.
          <div>
            <RecommendationVerificationLabel item={row} className="text-sm" />
            {recommendationDate(verification.observed_at)
              ? ` · ${recommendationDate(verification.observed_at)}`
              : ""}
          </div>
        ) : (
          t("capabilityCenter.recommended.verification.none")
        )}
      </DetailBlock>
      <DetailBlock title={t("capabilityCenter.recommended.sections.source")}>
        {row.source_uri ? (
          <a
            href={row.source_uri}
            target="_blank"
            rel="noopener noreferrer"
            className="underline underline-offset-2"
          >
            {recommendationSourceLabel(row.source_uri)}
          </a>
        ) : (
          t("capabilityCenter.emptyDash")
        )}
        {row.license ? (
          <div className="text-xs text-[var(--color-text-muted)]">{row.license}</div>
        ) : null}
      </DetailBlock>
    </>
  );
}

const MEMBER_SCOPE_ORDER = ["session", "actor", "group"] as const;

export function RecommendationMembers({ row }: { row: CapabilityOverviewItem }) {
  const { t } = useTranslation("settings");
  const members = row.members || [];
  const describe = (member: CapabilityMemberProjection): string[] => {
    const parts: string[] = [];
    if (member.blocked) {
      parts.push(t("capabilityCenter.recommended.member.blocked"));
    } else if (member.availability === "compatible") {
      parts.push(
        t(
          member.activation_pending
            ? "capabilityCenter.recommended.member.pending"
            : "capabilityCenter.recommended.member.compatible",
        ),
      );
    } else {
      const reasons = (member.reason_codes || [])
        .filter((code) => !code.startsWith("blocked_by_"))
        .map((code) => {
          const env = member.credential_env || "";
          const source = member.credential_source;
          // Say where to put the token when the daemon knows which setting it reads.
          if (code === "missing_member_credential" && (source === "actor" || source === "profile"))
            return t(`capabilityCenter.recommended.credential.${source}`, { env });
          return RECOMMENDATION_REASON_CODES.has(code)
            ? t(`capabilityCenter.recommended.reason.${code}`, { env })
            : code;
        });
      parts.push(
        reasons.length
          ? reasons.join(" · ")
          : t(`capabilityCenter.recommended.member.${member.availability}`),
      );
    }
    // Where it is enabled is a separate fact from whether the member can run it now.
    const scopes = member.enabled_scopes || [];
    for (const scope of MEMBER_SCOPE_ORDER) {
      if (scopes.includes(scope)) parts.push(t(`capabilityCenter.recommended.member.${scope}`));
    }
    if (member.last_call) {
      parts.push(
        t(
          member.last_call.ok
            ? "capabilityCenter.recommended.member.lastCallOk"
            : "capabilityCenter.recommended.member.lastCallFailed",
          { time: formatMessageTimestamp(member.last_call.at) },
        ),
      );
    }
    return parts;
  };
  return (
    <DetailBlock title={t("capabilityCenter.recommended.sections.members")}>
      {members.length === 0 ? (
        t("capabilityCenter.recommended.member.none")
      ) : (
        <ul className="space-y-1.5">
          {members.map((member) => (
            <li
              key={member.actor_id}
              className="grid grid-cols-[auto_minmax(0,1fr)] items-start gap-x-2"
              title={member.last_call ? formatFullTime(member.last_call.at) : undefined}
            >
              <RecommendationDotIcon
                dot={recommendationMemberDot(row, member)}
                className="mt-1.5"
              />
              <span className="min-w-0 break-words [overflow-wrap:anywhere]">
                <span className="font-medium text-[var(--color-text-primary)]">
                  {member.actor_id}
                </span>
                {` ${describe(member).join(" · ")}`}
              </span>
            </li>
          ))}
        </ul>
      )}
    </DetailBlock>
  );
}

function DetailBlock({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="min-w-0">
      <div className="text-xs font-semibold uppercase tracking-[0.08em] text-[var(--color-text-muted)]">
        {title}
      </div>
      <div className="mt-1 min-w-0 break-words text-sm leading-6 text-[var(--color-text-secondary)] [overflow-wrap:anywhere]">
        {children}
      </div>
    </section>
  );
}

function DetailList({ title, items }: { title: string; items: string[] }) {
  return (
    <section className="min-w-0">
      <div className="text-xs font-semibold uppercase tracking-[0.08em] text-[var(--color-text-muted)]">
        {title}
      </div>
      <ul className="mt-1 space-y-1 text-sm leading-6 text-[var(--color-text-secondary)]">
        {items.slice(0, 4).map((item, index) => (
          <li key={`${title}:${index}`} className="min-w-0 break-words [overflow-wrap:anywhere]">
            {item}
          </li>
        ))}
      </ul>
    </section>
  );
}
