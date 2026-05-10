import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type React from "react";
import { BookOpen, Boxes, Eye, EyeOff, Plug, Power, PowerOff, RefreshCcw, Search, Shield, SlidersHorizontal, Trash2, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import * as api from "../../services/api";
import { publishCapabilityChanged } from "../../utils/capabilityEvents";
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
import type { CapabilityOverviewItem, CapabilitySourceInstance, CapabilitySourceState, CapabilityStateResult } from "../../types";
import { CapabilityControlsPanel } from "./CapabilityControlsPanel";
import { SourcesSummary, SourcesView } from "./CapabilitySourcesView";
import {
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
  type CapabilityCenterSurface,
  type CapabilityCenterStateFilter,
  type CapabilityCenterStats,
} from "./capabilityCenterModel";
import { canManageSlashCommandVisibility, isCapabilityHiddenFromSlashCommands } from "../modals/settings/capabilityManagementModel";

interface CapabilityCenterWorkspaceProps {
  isOpen: boolean;
  onClose?: () => void;
  groupId?: string;
  isDark?: boolean;
  surface?: CapabilityCenterSurface;
}

const stateFilters: Array<{ id: CapabilityCenterStateFilter; label: string }> = [
  { id: "all", label: "All states" },
  { id: "enabled", label: "Enabled" },
  { id: "slash_visible", label: "/ visible" },
  { id: "slash_hidden", label: "/ hidden" },
  { id: "blocked", label: "Blocked" },
  { id: "needs_setup", label: "Needs setup" },
];

const CAPABILITY_CENTER_CLIENT_FILTER_LIMIT = 2000;

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
  label: string;
  hint: string;
}> = [
  { id: "skill", icon: BookOpen, label: "Skills", hint: "Procedural skills" },
  { id: "mcp", icon: Plug, label: "MCP", hint: "Tool servers" },
  { id: "sources", icon: Boxes, label: "Sources", hint: "Feeds and origins" },
];

function statusBadgeClass(kind: "neutral" | "good" | "warn" | "danger") {
  if (kind === "good") return "border-emerald-500/25 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
  if (kind === "warn") return "border-amber-500/28 bg-amber-500/12 text-amber-700 dark:text-amber-300";
  if (kind === "danger") return "border-rose-500/28 bg-rose-500/12 text-rose-700 dark:text-rose-300";
  return "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]";
}

function MiniBadge({ children, kind = "neutral" }: { children: React.ReactNode; kind?: "neutral" | "good" | "warn" | "danger" }) {
  return (
    <span className={`inline-flex h-6 items-center rounded border px-2 text-[11px] font-medium ${statusBadgeClass(kind)}`}>
      {children}
    </span>
  );
}

function TooltipIconButton({
  label,
  children,
}: {
  label: React.ReactNode;
  children: (referenceProps: Record<string, unknown>, setReference: (node: HTMLElement | null) => void) => React.ReactNode;
}) {
  return (
    <HoverTooltip label={label}>
      {(getReferenceProps, setReference) => children(getReferenceProps({ className: "inline-flex" }), setReference)}
    </HoverTooltip>
  );
}

export function CapabilityCenterWorkspace({ isOpen, onClose, groupId = "", isDark: _isDark, surface = "overlay" }: CapabilityCenterWorkspaceProps) {
  const { t } = useTranslation("settings");
  const [section, setSection] = useState<CapabilityCenterSection>("skill");
  const [query, setQuery] = useState("");
  const [stateFilter, setStateFilter] = useState<CapabilityCenterStateFilter>("all");
  const [showSystem, setShowSystem] = useState(false);
  const [items, setItems] = useState<CapabilityOverviewItem[]>([]);
  const [sources, setSources] = useState<Record<string, CapabilitySourceState>>({});
  const [sourceInstances, setSourceInstances] = useState<CapabilitySourceInstance[]>([]);
  const [state, setState] = useState<CapabilityStateResult | null>(null);
  const [summaryStats, setSummaryStats] = useState<CapabilityCenterStats | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const [stickyItems, setStickyItems] = useState<CapabilityOverviewItem[]>([]);
  const [removedCapabilityIds, setRemovedCapabilityIds] = useState<Set<string>>(() => new Set());
  const [pageIndex, setPageIndex] = useState(0);
  const [pageSize, setPageSize] = useState(CAPABILITY_CENTER_DEFAULT_PAGE_SIZE);
  const [totalCount, setTotalCount] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [busyKey, setBusyKey] = useState("");
  const [err, setErr] = useState("");
  const [mobileControlsOpen, setMobileControlsOpen] = useState(false);
  const [confirmDialog, setConfirmDialog] = useState<CapabilityCenterConfirmDialogState | null>(null);
  const requestSeqRef = useRef(0);

  const hiddenIds = useMemo(() => capabilityCenterHiddenIds(state), [state]);
  const loadedStats = useMemo(() => summarizeCapabilityCenter(items, state), [items, state]);
  const stats = summaryStats || loadedStats;
  const localizedSections = useMemo(() => sectionItems.map((item) => ({
    ...item,
    label: t(`capabilityCenter.sections.${item.id}.label`),
    hint: t(`capabilityCenter.sections.${item.id}.hint`),
  })), [t]);
  const localizedStateFilters = useMemo(() => stateFilters.map((item) => ({
    ...item,
    label: t(`capabilityCenter.stateOptions.${item.id}`),
  })), [t]);

  const scopedStateFilter = section === "sources" ? "all" : stateFilter;
  const scopedTypeFilter = capabilityCenterSectionTypeFilter(section);
  const paginationMode = useMemo(
    () => capabilityCenterPaginationMode({ section, stateFilter: scopedStateFilter }),
    [section, scopedStateFilter],
  );
  const clientPagination = paginationMode === "client";
  const filteredItems = useMemo(
    () => filterCapabilityCenterRemovedItems(
      filterCapabilityCenterItems(items, { query, typeFilter: scopedTypeFilter, stateFilter: scopedStateFilter, state }),
      removedCapabilityIds,
    ),
    [items, query, removedCapabilityIds, scopedStateFilter, scopedTypeFilter, state],
  );
  const baseMatchingItems = useMemo(
    () => capabilityCenterFilterItemsForSystemVisibility(filteredItems, {
      showSystem,
      query,
      state,
    }),
    [filteredItems, query, showSystem, state],
  );
  const stickyMatchingItems = useMemo(
    () => capabilityCenterFilterItemsForSystemVisibility(
      filterCapabilityCenterItems(stickyItems, {
        query,
        typeFilter: scopedTypeFilter,
        stateFilter: "all",
        state,
      }),
      { showSystem, query, state },
    ),
    [query, scopedTypeFilter, showSystem, state, stickyItems],
  );
  const matchingItems = useMemo(
    () => mergeCapabilityCenterStickyItems(baseMatchingItems, stickyMatchingItems),
    [baseMatchingItems, stickyMatchingItems],
  );
  const effectiveTotalCount = clientPagination ? matchingItems.length : totalCount;
  const normalizedPage = useMemo(
    () => normalizeCapabilityCenterPagination({ pageIndex, pageSize, totalCount: effectiveTotalCount }),
    [effectiveTotalCount, pageIndex, pageSize],
  );
  const visibleItems = useMemo(
    () => clientPagination
      ? matchingItems.slice(normalizedPage.offset, normalizedPage.offset + normalizedPage.pageSize)
      : matchingItems,
    [clientPagination, matchingItems, normalizedPage.offset, normalizedPage.pageSize],
  );
  const selected = useMemo(
    () => visibleItems.find((item) => String(item.capability_id || "") === selectedId) || visibleItems[0] || null,
    [selectedId, visibleItems],
  );
  const pageRange = useMemo(
    () => capabilityCenterPageRange({
      pageIndex: normalizedPage.pageIndex,
      pageSize: normalizedPage.pageSize,
      itemCount: visibleItems.length,
      totalCount: effectiveTotalCount,
    }),
    [effectiveTotalCount, normalizedPage.pageIndex, normalizedPage.pageSize, visibleItems.length],
  );

  const load = useCallback(async () => {
    const seq = ++requestSeqRef.current;
    const nextPageSize = normalizeCapabilityCenterPagination({ pageIndex: 0, pageSize, totalCount: 0 }).pageSize;
    const nextOffset = clientPagination ? 0 : Math.max(0, Math.trunc(Number(pageIndex) || 0)) * nextPageSize;
    const nextLimit = clientPagination ? CAPABILITY_CENTER_CLIENT_FILTER_LIMIT : nextPageSize;
    setLoading(true);
    setErr("");
    try {
      const overviewQuery = String(query || "").trim();
      const [overviewResp, stateResp, skillStatsResp, mcpStatsResp, packStatsResp] = await Promise.all([
        api.fetchCapabilityOverview({
          includeIndexed: true,
          limit: nextLimit,
          offset: nextOffset,
          query: overviewQuery || undefined,
          kind: scopedTypeFilter,
          policy: stateFilter === "blocked" ? "blocked" : "all",
          groupId,
        }),
        groupId ? api.fetchGroupCapabilityState(groupId, "user", { noCache: true }) : Promise.resolve(null),
        api.fetchCapabilityOverview({ includeIndexed: true, limit: 1, offset: 0, query: overviewQuery || undefined, kind: "skill", groupId }),
        api.fetchCapabilityOverview({ includeIndexed: true, limit: 1, offset: 0, query: overviewQuery || undefined, kind: "mcp", groupId }),
        api.fetchCapabilityOverview({ includeIndexed: true, limit: 1, offset: 0, query: overviewQuery || undefined, kind: "pack", groupId }),
      ]);
      if (seq !== requestSeqRef.current) return;
      if (!overviewResp.ok) {
        setErr(overviewResp.error?.message || t("capabilityCenter.failedLoad"));
        return;
      }
      if (stateResp && !stateResp.ok) {
        setErr(stateResp.error?.message || t("capabilityCenter.failedLoadState"));
      }
      const nextItems = overviewResp.result.items || [];
      const nextState = stateResp?.ok ? stateResp.result : null;
      const nextStats = summarizeCapabilityCenter(nextItems, nextState);
      setItems(nextItems);
      setTotalCount(Number(overviewResp.result.total_count || nextItems.length) || 0);
      setHasMore(Boolean(overviewResp.result.has_more));
      setSources(overviewResp.result.sources || {});
      setSourceInstances(overviewResp.result.source_instances || []);
      setState(nextState);
      const enabledCount = capabilityCenterEnabledIds(nextState).size;
      setSummaryStats({
        total: Number(overviewResp.result.total_count || nextItems.length) || 0,
        skills: skillStatsResp.ok ? Number(skillStatsResp.result.total_count || 0) : 0,
        mcp: mcpStatsResp.ok ? Number(mcpStatsResp.result.total_count || 0) : 0,
        packs: packStatsResp.ok ? Number(packStatsResp.result.total_count || 0) : 0,
        enabled: enabledCount,
        slashHidden: nextStats.slashHidden,
        blocked: nextStats.blocked,
        needsSetup: nextStats.needsSetup,
        sources: Object.keys(overviewResp.result.sources || {}).length,
      });
      setSelectedId((current) => current && nextItems.some((item) => item.capability_id === current) ? current : String(nextItems[0]?.capability_id || ""));
    } finally {
      if (seq === requestSeqRef.current) setLoading(false);
    }
  }, [clientPagination, groupId, pageIndex, pageSize, query, scopedTypeFilter, stateFilter, t]);

  useEffect(() => {
    if (!isOpen) return;
    void load();
  }, [isOpen, load]);

  useEffect(() => {
    setPageIndex(0);
  }, [query, section, stateFilter]);

  useEffect(() => {
    setStickyItems([]);
  }, [pageIndex, pageSize, query, section, showSystem, stateFilter]);

  const rememberStickyItem = useCallback((row: CapabilityOverviewItem) => {
    const capId = String(row.capability_id || "").trim();
    if (!capId) return;
    setStickyItems((current) => current
      .filter((item) => String(item.capability_id || "").trim() !== capId)
      .concat(row));
  }, []);

  const toggleSlashVisibility = useCallback(async (row: CapabilityOverviewItem) => {
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
      const stateResp = await api.fetchGroupCapabilityState(groupId, "user", { noCache: true });
      if (stateResp.ok) setState(stateResp.result);
      publishCapabilityChanged(groupId);
    } finally {
      setBusyKey("");
    }
  }, [groupId, hiddenIds, rememberStickyItem, t]);

  const executeRemoveCapability = useCallback(async (row: CapabilityOverviewItem, action: ReturnType<typeof capabilityCenterRemovalAction>) => {
    const capId = String(row.capability_id || "").trim();
    if (!groupId || !capId || action === "none") return;
    setBusyKey(`remove:${capId}`);
    setErr("");
    try {
      const resp = action === "disable"
        ? await api.enableGroupCapability(groupId, capId, {
          enabled: false,
          actorId: "user",
          reason: "capability center disable",
          cleanup: true,
        })
        : await api.uninstallCapability(groupId, capId, {
          actorId: "user",
          reason: `capability center ${action}`,
        });
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilityCenter.remove.failed"));
        return;
      }
      setRemovedCapabilityIds((current) => new Set([...current, capId]));
      await load();
      publishCapabilityChanged(groupId);
    } finally {
      setBusyKey("");
    }
  }, [groupId, load, t]);

  const removeCapability = useCallback((row: CapabilityOverviewItem, action: ReturnType<typeof capabilityCenterRemovalAction>) => {
    const capId = String(row.capability_id || "").trim();
    if (!groupId || !capId || action === "none") return;
    setConfirmDialog({
      title: t(`capabilityCenter.remove.confirmTitle.${action}`, { name: capabilityCenterDisplayName(row), id: capId }),
      description: t(`capabilityCenter.remove.confirmDescription.${action}`, { name: capabilityCenterDisplayName(row), id: capId }),
      confirmLabel: t(`capabilityCenter.remove.label.${action}`),
      tone: action === "disable" ? "default" : "destructive",
      onConfirm: () => executeRemoveCapability(row, action),
    });
  }, [executeRemoveCapability, groupId, t]);

  const executeBlockCapability = useCallback(async (row: CapabilityOverviewItem, nextBlocked: boolean) => {
    const capId = String(row.capability_id || "").trim();
    if (!groupId || !capId) return;
    setBusyKey(`block:${capId}`);
    setErr("");
    try {
      const resp = await api.blockCapabilityGlobal(capId, nextBlocked, "capability center policy update", groupId);
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
          : String(row.qualification_status || "").toLowerCase() === "blocked" ? "ready" : row.qualification_status,
      });
      await load();
      publishCapabilityChanged(groupId);
    } finally {
      setBusyKey("");
    }
  }, [groupId, load, rememberStickyItem, t]);

  const toggleBlockCapability = useCallback((row: CapabilityOverviewItem) => {
    const capId = String(row.capability_id || "").trim();
    if (!groupId || !capId) return;
    const blocked = capabilityCenterIsBlocked(row);
    setConfirmDialog({
      title: t(blocked ? "capabilityCenter.block.confirmUnblockTitle" : "capabilityCenter.block.confirmBlockTitle", {
        name: capabilityCenterDisplayName(row),
        id: capId,
      }),
      description: t(blocked ? "capabilityCenter.block.confirmUnblockDescription" : "capabilityCenter.block.confirmBlockDescription", {
        name: capabilityCenterDisplayName(row),
        id: capId,
      }),
      confirmLabel: t(blocked ? "capabilityCenter.block.unblock" : "capabilityCenter.block.block"),
      tone: blocked ? "default" : "destructive",
      onConfirm: () => executeBlockCapability(row, !blocked),
    });
  }, [executeBlockCapability, groupId, t]);

  const executeEnableCapability = useCallback(async (row: CapabilityOverviewItem, nextEnabled: boolean) => {
    const capId = String(row.capability_id || "").trim();
    if (!groupId || !capId) return;
    setBusyKey(`enable:${capId}`);
    setErr("");
    try {
      const resp = await api.enableGroupCapability(groupId, capId, {
        enabled: nextEnabled,
        scope: "group",
        actorId: "user",
        reason: nextEnabled ? "capability center enable" : "capability center disable",
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilityCenter.enable.failed"));
        return;
      }
      rememberStickyItem(row);
      await load();
      publishCapabilityChanged(groupId);
    } finally {
      setBusyKey("");
    }
  }, [groupId, load, rememberStickyItem, t]);

  const toggleEnableCapability = useCallback((row: CapabilityOverviewItem) => {
    const capId = String(row.capability_id || "").trim();
    if (!groupId || !capId) return;
    const enabledIds = capabilityCenterEnabledIds(state);
    const enabled = enabledIds.has(capId);
    if (!enabled) {
      void executeEnableCapability(row, true);
      return;
    }
    setConfirmDialog({
      title: t("capabilityCenter.enable.confirmDisableTitle", { name: capabilityCenterDisplayName(row), id: capId }),
      description: t("capabilityCenter.enable.confirmDisableDescription", { name: capabilityCenterDisplayName(row), id: capId }),
      confirmLabel: t("capabilityCenter.enable.disable"),
      onConfirm: () => executeEnableCapability(row, false),
    });
  }, [executeEnableCapability, groupId, state, t]);

  const toggleSource = useCallback(async (sourceId: string, nextEnabled: boolean) => {
    const sid = String(sourceId || "").trim();
    if (!sid) return;
    setBusyKey(`source:${sid}`);
    setErr("");
    try {
      const patchSources = Object.values(sources || {})
        .map((row) => ({
          source_id: String(row.source_id || "").trim(),
          enabled: String(row.source_id || "").trim() === sid ? nextEnabled : Boolean(row.enabled),
          rationale: String(row.rationale || ""),
        }))
        .filter((row) => row.source_id);
      if (!patchSources.some((row) => row.source_id === sid)) {
        patchSources.push({ source_id: sid, enabled: nextEnabled, rationale: "" });
      }
      const resp = await api.updateCapabilityAllowlist({ patch: { sources: patchSources } });
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilityCenter.sources.toggleFailed"));
        return;
      }
      await load();
      publishCapabilityChanged(groupId);
    } finally {
      setBusyKey("");
    }
  }, [groupId, load, sources, t]);

  const deleteSource = useCallback(async (source: CapabilitySourceState) => {
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
      await load();
      publishCapabilityChanged(groupId);
    } finally {
      setBusyKey("");
    }
  }, [groupId, load, t]);

  const confirmDeleteSource = useCallback((source: CapabilitySourceState) => {
    const sourceId = String(source.source_id || "").trim();
    if (!groupId || !sourceId || capabilityCenterSourceRemovalAction(source) === "none") return;
    setConfirmDialog({
      title: t("capabilityCenter.sources.deleteConfirmTitle", { id: sourceId, count: Number(source.record_count || 0) }),
      description: t("capabilityCenter.sources.deleteConfirmDescription", { id: sourceId, count: Number(source.record_count || 0) }),
      confirmLabel: t("capabilityCenter.sources.delete"),
      tone: "destructive",
      onConfirm: () => deleteSource(source),
    });
  }, [deleteSource, groupId, t]);

  const deleteSourceInstance = useCallback(async (instance: CapabilitySourceInstance) => {
    const sourceId = String(instance.source_id || "").trim();
    const sourceInstanceKey = String(instance.source_instance_key || "").trim();
    if (!groupId || !sourceId || !sourceInstanceKey) return;
    setBusyKey(`source-instance-delete:${sourceInstanceKey}`);
    setErr("");
    try {
      const resp = await api.deleteCapabilitySource(groupId, sourceId, {
        actorId: "user",
        sourceInstanceKey,
        reason: "capability center source instance delete",
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("capabilityCenter.sources.deleteFailed"));
        return;
      }
      await load();
      publishCapabilityChanged(groupId);
    } finally {
      setBusyKey("");
    }
  }, [groupId, load, t]);

  const confirmDeleteSourceInstance = useCallback((instance: CapabilitySourceInstance) => {
    const sourceId = String(instance.source_id || "").trim();
    const sourceInstanceKey = String(instance.source_instance_key || "").trim();
    if (!groupId || !sourceId || !sourceInstanceKey) return;
    setConfirmDialog({
      title: t("capabilityCenter.sources.deleteInstanceConfirmTitle", {
        name: instance.label || sourceInstanceKey,
        count: Number(instance.record_count || 0),
      }),
      description: t("capabilityCenter.sources.deleteInstanceConfirmDescription", {
        name: instance.label || sourceInstanceKey,
        count: Number(instance.record_count || 0),
      }),
      confirmLabel: t("capabilityCenter.sources.delete"),
      tone: "destructive",
      onConfirm: () => deleteSourceInstance(instance),
    });
  }, [deleteSourceInstance, groupId, t]);

  if (!isOpen) return null;

  const rootClass = capabilityCenterRootClass(surface);
  const frameClass = capabilityCenterFrameClass(surface);

  return (
    <div className={rootClass} role={surface === "overlay" ? "dialog" : undefined} aria-modal={surface === "overlay" ? true : undefined} aria-label={t("capabilityCenter.title")}>
      <div className={frameClass}>
      <aside className="hidden w-[248px] shrink-0 border-r border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] md:flex md:flex-col">
        <div className="border-b border-[var(--glass-border-subtle)] px-4 py-4">
          <div className="text-sm font-semibold">{t("capabilityCenter.title")}</div>
          <div className="mt-1 text-xs text-[var(--color-text-muted)]">{t("capabilityCenter.subtitle")}</div>
        </div>
        <nav className="flex-1 space-y-1 overflow-y-auto px-2 py-3">
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
                onClick={() => setSection(item.id)}
              >
                <Icon size={17} aria-hidden="true" />
                <span className="min-w-0">
                  <span className="block text-sm font-medium">{item.label}</span>
                  <span className="block truncate text-[11px] text-[var(--color-text-muted)]">{item.hint}</span>
                </span>
              </button>
            );
          })}
        </nav>
        <div className="border-t border-[var(--glass-border-subtle)] px-4 py-3 text-xs text-[var(--color-text-muted)]">
          {t("capabilityCenter.sidebarSummary", { capabilities: totalCount || stats.total, sources: Object.keys(sources || {}).length })}
        </div>
      </aside>

      <main className="flex min-h-0 min-w-0 flex-1 flex-col">
        <header className="flex min-h-[56px] items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-3 py-2 sm:px-4 sm:py-3 lg:px-6">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <BookOpen size={18} aria-hidden="true" />
              <h2 className="min-w-0 truncate text-base font-semibold sm:text-lg">{t("capabilityCenter.title")}</h2>
            </div>
            <p className="mt-0.5 hidden truncate text-xs text-[var(--color-text-muted)] sm:block">{t("capabilityCenter.headerHint")}</p>
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
                void load();
              }}
            >
              <RefreshCcw size={15} aria-hidden="true" />
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
                      <X size={17} aria-hidden="true" />
                    </Button>
                  </span>
                )}
              </TooltipIconButton>
            ) : (
              <Button asChild variant="outline" size="icon" className="h-10 w-10 bg-[var(--glass-panel-bg)] sm:w-auto sm:px-3">
                <a href="/ui/">
                  <X size={15} aria-hidden="true" />
                  <span className="sr-only sm:not-sr-only sm:truncate">{t("capabilityCenter.backToApp")}</span>
                </a>
              </Button>
            )}
          </div>
        </header>

        <nav className="flex gap-2 overflow-x-auto border-b border-[var(--glass-border-subtle)] px-3 py-2 md:hidden [scrollbar-width:none] [&::-webkit-scrollbar]:hidden" aria-label={t("capabilityCenter.sectionNavigation")}>
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
                onClick={() => setSection(item.id)}
              >
                <Icon size={15} aria-hidden="true" />
                <span>{item.label}</span>
              </button>
            );
          })}
        </nav>

        <div className="grid min-h-0 flex-1 grid-cols-1 lg:grid-cols-[minmax(0,1fr)_360px]">
          <section className="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden border-r border-[var(--glass-border-subtle)]">
            <div className="min-w-0 border-b border-[var(--glass-border-subtle)] px-3 py-2 sm:px-4 sm:py-3 lg:px-6">
              <div className="hidden grid-cols-2 gap-2 sm:grid sm:grid-cols-4">
                <Stat label={t("capabilityCenter.stats.skills")} value={stats.skills} />
                <Stat label={t("capabilityCenter.stats.mcp")} value={stats.mcp} />
                <Stat label={t("capabilityCenter.stats.packs")} value={stats.packs} />
                <Stat label={t("capabilityCenter.stats.enabled")} value={stats.enabled} />
              </div>
              <div className="flex items-center gap-2 sm:hidden">
                <button
                  type="button"
                  className="inline-flex min-h-[36px] items-center gap-2 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 text-xs font-medium text-[var(--color-text-secondary)]"
                  aria-expanded={mobileControlsOpen}
                  onClick={() => setMobileControlsOpen((value) => !value)}
                >
                  <SlidersHorizontal size={14} aria-hidden="true" />
                  <span>{t("capabilityCenter.stateFilter")}</span>
                </button>
                <span className="min-w-0 truncate text-xs text-[var(--color-text-muted)]">
                  {section === "sources"
                    ? t("capabilityCenter.sections.sources.label")
                    : `${t(`capabilityCenter.stateOptions.${scopedStateFilter}`)} · ${pageRange.total || totalCount || stats.total}`}
                </span>
              </div>
              <div className={`${mobileControlsOpen ? "mt-2 flex" : "hidden"} flex-col gap-2 sm:mt-3 sm:flex xl:flex-row xl:items-center`}>
                <div className="relative min-w-0 flex-1">
                  <Search className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-[var(--color-text-muted)]" size={15} aria-hidden="true" />
                  <input
                    className="h-9 w-full rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] pl-9 pr-3 text-sm outline-none focus:border-[rgba(59,130,246,0.42)] sm:h-10"
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
                    disabled={section === "sources"}
                    onChange={(value) => setStateFilter(value as CapabilityCenterStateFilter)}
                  />
                  {section !== "sources" ? (
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
                  ) : null}
                </div>
              </div>
              {err ? <div className="mt-2 rounded-lg border border-rose-500/25 bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">{err}</div> : null}
            </div>

            {section === "sources" ? (
              <SourcesView
                sources={sources}
                sourceInstances={sourceInstances}
                busyKey={busyKey}
                onToggle={toggleSource}
                onDelete={confirmDeleteSource}
                onDeleteInstance={confirmDeleteSourceInstance}
              />
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
            <SourcesSummary sources={sources} sourceInstances={sourceInstances} />
          ) : (
            <CapabilityDetails
              item={selected}
              state={state}
              hiddenIds={hiddenIds}
              groupId={groupId}
              busyKey={busyKey}
              onToggleSlash={toggleSlashVisibility}
              onToggleBlock={toggleBlockCapability}
              onToggleEnable={toggleEnableCapability}
              onRemove={removeCapability}
            />
          )}
        </div>
      </main>
      <Dialog open={Boolean(confirmDialog)} onOpenChange={(open) => {
        if (!open && !busyKey) setConfirmDialog(null);
      }}>
        <DialogContent className="w-[min(calc(100vw-1.5rem),28rem)] p-5">
          <DialogHeader>
            <DialogTitle>{confirmDialog?.title || ""}</DialogTitle>
            <DialogDescription className="leading-6">
              {confirmDialog?.description || ""}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="mt-5">
            <Button type="button" variant="outline" onClick={() => setConfirmDialog(null)} disabled={Boolean(busyKey)}>
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
      <div className={`flex max-w-full items-center gap-1 overflow-x-auto rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden ${props.disabled ? "opacity-50" : ""}`}>
        <SlidersHorizontal size={14} className="ml-1 shrink-0 text-[var(--color-text-muted)]" aria-hidden="true" />
        {props.options.map((option) => (
          <button
            key={option.id}
            type="button"
            className={`min-h-[34px] shrink-0 rounded-md px-3 text-xs font-medium sm:min-h-[36px] ${props.value === option.id ? "bg-[var(--glass-bg-active)] text-[var(--color-text-primary)]" : "text-[var(--color-text-secondary)] hover:bg-[var(--glass-bg-hover)]"}`}
            disabled={props.disabled}
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
  onRemove: (row: CapabilityOverviewItem, action: ReturnType<typeof capabilityCenterRemovalAction>) => void;
  onToggleBlock: (row: CapabilityOverviewItem) => void;
  onToggleEnable: (row: CapabilityOverviewItem) => void;
}) {
  const { t } = useTranslation("settings");
  const enabledIds = useMemo(() => new Set([...(props.state?.enabled_capabilities || []), ...(props.state?.enabled || []).map((item) => item.capability_id)]), [props.state]);

  if (props.loading && props.items.length === 0) {
    return <div className="p-6 text-sm text-[var(--color-text-muted)]">{t("capabilityCenter.loading")}</div>;
  }

  return (
    <div className="h-full min-h-0 overflow-auto scrollbar-subtle">
      <div className="hidden md:block">
        <div className="sticky top-0 z-[1] grid min-w-[980px] grid-cols-[minmax(260px,1.8fr)_88px_150px_130px_170px] gap-3 border-b border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-4 py-2 text-[11px] font-semibold uppercase tracking-[0.06em] text-[var(--color-text-muted)] lg:px-6">
          <div>{t("capabilityCenter.table.capability")}</div>
          <div>{t("capabilityCenter.table.type")}</div>
          <div>{t("capabilityCenter.table.state")}</div>
          <div>{t("capabilityCenter.table.source")}</div>
          <div>{t("capabilityCenter.table.actions")}</div>
        </div>
        <div className="min-w-[860px] divide-y divide-[var(--glass-border-subtle)]">
          {props.items.map((row) => {
            const capId = String(row.capability_id || "").trim();
            const selected = capId === props.selectedId;
            const type = capabilityCenterType(row);
            const enabled = enabledIds.has(capId);
            const hidden = isCapabilityHiddenFromSlashCommands(capId, props.hiddenIds);
            const canShowSlashToggle = enabled && canManageSlashCommandVisibility(row);
            const blocked = capabilityCenterIsBlocked(row);
            const needsSetup = capabilityCenterNeedsSetup(row);
            const removalAction = capabilityCenterRemovalAction(row, { enabled });
            return (
              <div
                key={capId}
                role="button"
                tabIndex={0}
                className={`grid w-full grid-cols-[minmax(260px,1.8fr)_88px_150px_130px_170px] items-center gap-3 px-4 py-2.5 text-left text-sm lg:px-6 ${
                  selected ? "bg-[var(--glass-bg-active)]" : "hover:bg-[var(--glass-bg-hover)]"
                }`}
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
                <div><MiniBadge>{capabilityCenterTypeLabel(type)}</MiniBadge></div>
                <div className="flex flex-wrap gap-1">
                  {enabled ? <MiniBadge kind="good">{t("capabilityCenter.status.enabled")}</MiniBadge> : null}
                  {blocked ? <MiniBadge kind="danger">{t("capabilityCenter.status.blocked")}</MiniBadge> : needsSetup ? <MiniBadge kind="warn">{t("capabilityCenter.status.setup")}</MiniBadge> : <MiniBadge>{t("capabilityCenter.status.ready")}</MiniBadge>}
                </div>
                <div className="truncate text-xs text-[var(--color-text-secondary)]">{row.source_id || "-"}</div>
                <CapabilityRowActions
                  row={row}
                  enabled={enabled}
                  hidden={hidden}
                  blocked={blocked}
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
          {props.items.length === 0 ? <div className="px-6 py-10 text-sm text-[var(--color-text-muted)]">{t("capabilityCenter.noCapabilities")}</div> : null}
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
          const removalAction = capabilityCenterRemovalAction(row, { enabled });
          return (
            <div
              key={capId}
              role="button"
              tabIndex={0}
              className={`rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-3 text-left text-sm ${
                selected ? "ring-1 ring-[rgba(59,130,246,0.42)]" : ""
              }`}
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
                  <div className="break-words font-medium [overflow-wrap:anywhere]">{capabilityCenterDisplayName(row)}</div>
                  <div className="mt-0.5 break-words text-xs text-[var(--color-text-muted)] [overflow-wrap:anywhere]">{capId}</div>
                </div>
                <MiniBadge>{capabilityCenterTypeLabel(type)}</MiniBadge>
              </div>
              <div className="mt-2 flex flex-wrap gap-1.5">
                {enabled ? <MiniBadge kind="good">{t("capabilityCenter.status.enabled")}</MiniBadge> : null}
                {blocked ? <MiniBadge kind="danger">{t("capabilityCenter.status.blocked")}</MiniBadge> : needsSetup ? <MiniBadge kind="warn">{t("capabilityCenter.status.setup")}</MiniBadge> : <MiniBadge>{t("capabilityCenter.status.ready")}</MiniBadge>}
                {row.source_id ? <MiniBadge>{row.source_id}</MiniBadge> : null}
              </div>
              <CapabilityRowActions
                row={row}
                enabled={enabled}
                hidden={hidden}
                blocked={blocked}
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
        {props.items.length === 0 ? <div className="px-6 py-10 text-sm text-[var(--color-text-muted)]">{t("capabilityCenter.noCapabilities")}</div> : null}
      </div>
    </div>
  );
}

function CapabilityRowActions(props: {
  row: CapabilityOverviewItem;
  enabled: boolean;
  hidden: boolean;
  blocked: boolean;
  canShowSlashToggle: boolean;
  removalAction: ReturnType<typeof capabilityCenterRemovalAction>;
  busyKey: string;
  compact?: boolean;
  onToggleSlash: (row: CapabilityOverviewItem) => void;
  onRemove: (row: CapabilityOverviewItem, action: ReturnType<typeof capabilityCenterRemovalAction>) => void;
  onToggleBlock: (row: CapabilityOverviewItem) => void;
  onToggleEnable: (row: CapabilityOverviewItem) => void;
}) {
  const { t } = useTranslation("settings");
  const capId = String(props.row.capability_id || "").trim();
  return (
    <div className={cn("flex gap-1.5", props.compact ? "mt-3 justify-start" : "justify-end")}>
      <EnableCapabilityButton
        enabled={props.enabled}
        busy={props.busyKey === `enable:${capId}`}
        compact={props.compact}
        onClick={(event) => {
          event.stopPropagation();
          props.onToggleEnable(props.row);
        }}
      />
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
        <span className="self-center text-xs text-[var(--color-text-muted)]">{t("capabilityCenter.emptyDash")}</span>
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
            ? t("capabilityCenter.pagination.showing", { from: props.rangeFrom, to: props.rangeTo, total: props.totalCount })
            : t("capabilityCenter.pagination.noResults")}
        </span>
        {props.hasMore ? <MiniBadge>{t("capabilityCenter.pagination.moreAvailable")}</MiniBadge> : null}
      </div>
      <div className="grid gap-2 sm:flex sm:items-center">
        <label className="hidden items-center gap-2 sm:flex">
          <span>{t("capabilityCenter.pagination.rows")}</span>
          <select
            className="h-9 rounded-md border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-2 text-xs text-[var(--color-text-primary)] sm:h-8"
            value={props.pageSize}
            disabled={props.loading}
            onChange={(event) => props.onPageSizeChange(Number(event.target.value))}
          >
            {CAPABILITY_CENTER_PAGE_SIZE_OPTIONS.map((size) => (
              <option key={size} value={size}>{size}</option>
            ))}
          </select>
        </label>
        <div className="grid grid-cols-[minmax(0,1fr)_64px_minmax(0,1fr)] items-center gap-2 sm:flex">
          <button
            type="button"
            className="inline-flex min-h-[36px] items-center justify-center rounded-md border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 text-xs hover:bg-[var(--glass-bg-hover)] disabled:cursor-not-allowed disabled:opacity-50 sm:h-8 sm:min-h-0"
            disabled={!canPrev}
            onClick={() => props.onPageIndexChange(Math.max(0, props.pageIndex - 1))}
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
            onClick={() => props.onPageIndexChange(Math.min(props.totalPages - 1, props.pageIndex + 1))}
          >
            {t("capabilityCenter.pagination.next")}
          </button>
        </div>
      </div>
    </div>
  );
}

function EnableCapabilityButton({ enabled, busy, compact, onClick }: { enabled: boolean; busy: boolean; compact?: boolean; onClick: (event: React.MouseEvent<HTMLButtonElement>) => void }) {
  const { t } = useTranslation("settings");
  const Icon = enabled ? PowerOff : Power;
  const label = enabled ? t("capabilityCenter.enable.disable") : t("capabilityCenter.enable.enable");
  const tooltip = enabled ? t("capabilityCenter.enable.disableTitle") : t("capabilityCenter.enable.enableTitle");
  return (
    <TooltipIconButton label={tooltip}>
      {(referenceProps, setReference) => (
        <span ref={setReference} {...referenceProps}>
          <button
            type="button"
            className={cn("inline-flex items-center justify-center rounded-md border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-primary)] hover:bg-[var(--glass-bg-hover)] disabled:opacity-60", compact ? "h-10 w-10" : "h-8 w-8", busy ? "cursor-not-allowed opacity-60" : "")}
            disabled={busy}
            aria-label={label}
            onClick={onClick}
          >
            <Icon size={14} aria-hidden="true" />
          </button>
        </span>
      )}
    </TooltipIconButton>
  );
}

function IconToggle({ hidden, busy, compact, onClick }: { hidden: boolean; busy: boolean; compact?: boolean; onClick: (event: React.MouseEvent<HTMLButtonElement>) => void }) {
  const { t } = useTranslation("settings");
  const Icon = hidden ? EyeOff : Eye;
  const label = hidden ? t("capabilityCenter.slashHidden") : t("capabilityCenter.slashVisible");
  const tooltip = hidden ? t("capabilityCenter.showInSlashCommands") : t("capabilityCenter.hideFromSlashCommands");
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
          >
            <Icon size={14} aria-hidden="true" />
          </button>
        </span>
      )}
    </TooltipIconButton>
  );
}

function BlockCapabilityButton({ blocked, busy, compact, onClick }: { blocked: boolean; busy: boolean; compact?: boolean; onClick: (event: React.MouseEvent<HTMLButtonElement>) => void }) {
  const { t } = useTranslation("settings");
  const label = blocked ? t("capabilityCenter.block.unblock") : t("capabilityCenter.block.block");
  const tooltip = blocked ? t("capabilityCenter.block.unblockTitle") : t("capabilityCenter.block.blockTitle");
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
          >
            <Shield size={15} aria-hidden="true" />
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
  const label = busy ? t("capabilityCenter.remove.busy") : t(`capabilityCenter.remove.label.${action}`);
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
          >
            <Trash2 size={14} aria-hidden="true" />
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
  busyKey: string;
  onToggleSlash: (row: CapabilityOverviewItem) => void;
  onToggleBlock: (row: CapabilityOverviewItem) => void;
  onToggleEnable: (row: CapabilityOverviewItem) => void;
  onRemove: (row: CapabilityOverviewItem, action: Exclude<ReturnType<typeof capabilityCenterRemovalAction>, "none">) => void;
}) {
  const { t } = useTranslation("settings");
  if (!props.item) {
    return (
      <aside className="hidden min-h-0 min-w-0 overflow-x-hidden overflow-y-auto p-5 text-sm text-[var(--color-text-muted)] lg:block">
        {t("capabilityCenter.details.selectCapability")}
      </aside>
    );
  }
  const row = props.item;
  const capId = String(row.capability_id || "").trim();
  const type = capabilityCenterType(row);
  const enabledIds = new Set([...(props.state?.enabled_capabilities || []), ...(props.state?.enabled || []).map((item) => item.capability_id)]);
  const enabled = enabledIds.has(capId);
  const hidden = isCapabilityHiddenFromSlashCommands(capId, props.hiddenIds);
  const canShowSlashToggle = enabled && canManageSlashCommandVisibility(row);
  const blocked = capabilityCenterIsBlocked(row);
  const needsSetup = capabilityCenterNeedsSetup(row);
  const preview = row.readiness_preview;
  const removalAction = capabilityCenterRemovalAction(row, { enabled });

  return (
    <aside className="hidden min-h-0 min-w-0 overflow-x-hidden overflow-y-auto bg-[var(--glass-panel-bg)] p-5 lg:block">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-xs uppercase tracking-[0.08em] text-[var(--color-text-muted)]">{capabilityCenterTypeLabel(type)}</div>
          <h3 className="mt-1 break-words text-lg font-semibold [overflow-wrap:anywhere]">{capabilityCenterDisplayName(row)}</h3>
          <p className="mt-1 break-all text-xs text-[var(--color-text-muted)]">{capId}</p>
        </div>
        <MiniBadge kind={blocked ? "danger" : needsSetup ? "warn" : enabled ? "good" : "neutral"}>
          {blocked ? t("capabilityCenter.status.blocked") : needsSetup ? t("capabilityCenter.status.needsSetup") : enabled ? t("capabilityCenter.status.enabled") : t("capabilityCenter.status.ready")}
        </MiniBadge>
      </div>

      <div className="mt-5 space-y-4">
        <DetailBlock title={t("capabilityCenter.details.description")}>{row.description_short || t("capabilityCenter.details.noDescription")}</DetailBlock>
        <DetailBlock title={t("capabilityCenter.details.source")}>{row.source_id || t("capabilityCenter.emptyDash")}{row.source_uri ? ` · ${row.source_uri}` : ""}</DetailBlock>
        <DetailBlock title={t("capabilityCenter.details.policy")}>{row.policy_level || t("capabilityCenter.emptyDash")}{row.qualification_status ? ` · ${row.qualification_status}` : ""}</DetailBlock>
        {preview ? (
          <DetailBlock title={t("capabilityCenter.details.readiness")}>
            {[preview.preview_status, preview.next_step, preview.enable_block_reason].filter(Boolean).join(" · ") || t("capabilityCenter.emptyDash")}
          </DetailBlock>
        ) : null}
        {row.tool_names?.length ? <DetailBlock title={t("capabilityCenter.details.tools")}>{row.tool_names.join(", ")}</DetailBlock> : null}
        {row.use_when?.length ? <DetailList title={t("capabilityCenter.details.useWhen")} items={row.use_when} /> : null}
        {row.gotchas?.length ? <DetailList title={t("capabilityCenter.details.gotchas")} items={row.gotchas} /> : null}
      </div>

      <CapabilityControlsPanel
        row={row}
        enabled={enabled}
        hidden={hidden}
        blocked={blocked}
        canShowSlashToggle={canShowSlashToggle}
        removalAction={removalAction}
        busyKey={props.busyKey}
        groupId={props.groupId}
        onToggleEnable={props.onToggleEnable}
        onToggleSlash={props.onToggleSlash}
        onToggleBlock={props.onToggleBlock}
        onRemove={props.onRemove}
      />
    </aside>
  );
}

function DetailBlock({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="min-w-0">
      <div className="text-xs font-semibold uppercase tracking-[0.08em] text-[var(--color-text-muted)]">{title}</div>
      <div className="mt-1 min-w-0 break-words text-sm leading-6 text-[var(--color-text-secondary)] [overflow-wrap:anywhere]">{children}</div>
    </section>
  );
}

function DetailList({ title, items }: { title: string; items: string[] }) {
  return (
    <section className="min-w-0">
      <div className="text-xs font-semibold uppercase tracking-[0.08em] text-[var(--color-text-muted)]">{title}</div>
      <ul className="mt-1 space-y-1 text-sm leading-6 text-[var(--color-text-secondary)]">
        {items.slice(0, 4).map((item, index) => (
          <li key={`${title}:${index}`} className="min-w-0 break-words [overflow-wrap:anywhere]">{item}</li>
        ))}
      </ul>
    </section>
  );
}
