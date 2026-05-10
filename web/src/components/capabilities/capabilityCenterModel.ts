import type { CapabilityOverviewItem, CapabilitySourceState, CapabilityStateResult } from "../../types";
import { canManageSlashCommandVisibility, isCapabilityHiddenFromSlashCommands } from "../modals/settings/capabilityManagementModel";

export type CapabilityCenterSection = "skill" | "mcp" | "sources";
export type CapabilityCenterTypeFilter = "all" | "skill" | "mcp" | "pack";
export type CapabilityCenterStateFilter = "all" | "enabled" | "slash_visible" | "slash_hidden" | "blocked" | "needs_setup";
export type CapabilityCenterRemovalAction = "delete" | "remove" | "uninstall" | "disable" | "none";
export type CapabilityCenterSourceRemovalAction = "delete" | "none";
export type CapabilityCenterSurface = "overlay" | "page";

export type CapabilityCenterStats = {
  total: number;
  skills: number;
  mcp: number;
  packs: number;
  enabled: number;
  slashHidden: number;
  blocked: number;
  needsSetup: number;
  sources: number;
};

export const CAPABILITY_CENTER_DEFAULT_PAGE_SIZE = 80;
export const CAPABILITY_CENTER_PAGE_SIZE_OPTIONS = [40, 80, 120] as const;

export function capabilityCenterSectionTypeFilter(section: CapabilityCenterSection): CapabilityCenterTypeFilter {
  if (section === "skill") return "skill";
  if (section === "mcp") return "mcp";
  return "all";
}

export function capabilityCenterRemovalAction(
  row: Pick<CapabilityOverviewItem, "capability_id" | "kind" | "source_id" | "cached_install_state" | "recent_success">,
  input?: { enabled?: boolean },
): CapabilityCenterRemovalAction {
  const sourceId = String(row.source_id || "").trim();
  if (!sourceId) return "none";
  if (sourceId === "cccc_builtin") return input?.enabled ? "disable" : "none";
  const type = capabilityCenterType(row);
  if (type === "skill" && sourceId === "agent_self_proposed") return "delete";
  const hasLocalFootprint = Boolean(
    input?.enabled
      || String(row.cached_install_state || "").trim()
      || row.recent_success,
  );
  if (!hasLocalFootprint) return "none";
  if (type === "skill") return "remove";
  return "uninstall";
}

export function capabilityCenterSourceRemovalAction(source: Pick<CapabilitySourceState, "source_id">): CapabilityCenterSourceRemovalAction {
  const sourceId = String(source.source_id || "").trim();
  if (["manual_import", "github_import", "url_import", "local_import", "agent_self_proposed"].includes(sourceId)) return "delete";
  return "none";
}

export function capabilityCenterSourcesGridClass(): string {
  return "grid grid-cols-[minmax(0,1fr)_76px] items-start gap-3 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 py-3 text-sm md:grid-cols-[minmax(220px,1fr)_110px_110px_minmax(150px,1fr)_76px] md:items-center md:py-2";
}

export function capabilityCenterRootClass(surface: CapabilityCenterSurface): string {
  if (surface === "page") return "h-dvh w-full overflow-hidden bg-[var(--color-bg-primary)] text-[var(--color-text-primary)]";
  return "fixed inset-0 z-[120] flex overflow-hidden bg-[var(--color-bg-primary)] text-[var(--color-text-primary)]";
}

export function capabilityCenterFrameClass(surface: CapabilityCenterSurface): string {
  if (surface === "page") return "flex h-dvh w-full";
  return "flex min-h-0 w-full";
}

export function capabilityCenterPaginationMode(input: {
  section: CapabilityCenterSection;
  stateFilter: CapabilityCenterStateFilter;
}): "server" | "client" {
  if (["enabled", "slash_visible", "slash_hidden", "needs_setup"].includes(input.stateFilter)) return "client";
  return "server";
}

export function normalizeCapabilityCenterPagination(input: {
  pageIndex: number;
  pageSize: number;
  totalCount: number;
}): {
  pageIndex: number;
  pageSize: number;
  totalPages: number;
  offset: number;
} {
  const pageSizeCandidate = Math.trunc(Number(input.pageSize) || CAPABILITY_CENTER_DEFAULT_PAGE_SIZE);
  const pageSize = CAPABILITY_CENTER_PAGE_SIZE_OPTIONS.includes(pageSizeCandidate as (typeof CAPABILITY_CENTER_PAGE_SIZE_OPTIONS)[number])
    ? pageSizeCandidate
    : CAPABILITY_CENTER_DEFAULT_PAGE_SIZE;
  const totalCount = Math.max(0, Math.trunc(Number(input.totalCount) || 0));
  const totalPages = Math.max(1, Math.ceil(totalCount / pageSize));
  const pageIndex = Math.min(
    totalPages - 1,
    Math.max(0, Math.trunc(Number(input.pageIndex) || 0)),
  );
  return {
    pageIndex,
    pageSize,
    totalPages,
    offset: pageIndex * pageSize,
  };
}

export function capabilityCenterPageRange(input: {
  pageIndex: number;
  pageSize: number;
  itemCount: number;
  totalCount: number;
}): {
  from: number;
  to: number;
  total: number;
} {
  const page = normalizeCapabilityCenterPagination(input);
  const itemCount = Math.max(0, Math.trunc(Number(input.itemCount) || 0));
  const total = Math.max(0, Math.trunc(Number(input.totalCount) || 0));
  if (!itemCount || !total) return { from: 0, to: 0, total };
  const from = page.offset + 1;
  return {
    from,
    to: Math.min(total, page.offset + itemCount),
    total,
  };
}

export function capabilityCenterType(row: Pick<CapabilityOverviewItem, "kind" | "capability_id">): "skill" | "mcp" | "pack" {
  const kind = String(row.kind || "").trim().toLowerCase();
  const capId = String(row.capability_id || "").trim().toLowerCase();
  if (kind === "mcp_toolpack" || kind === "mcp" || capId.startsWith("mcp:")) return "mcp";
  if (kind === "pack" || capId.startsWith("pack:")) return "pack";
  return "skill";
}

export function capabilityCenterTypeLabel(type: "skill" | "mcp" | "pack"): string {
  if (type === "mcp") return "MCP";
  if (type === "pack") return "Pack";
  return "Skill";
}

export function capabilityCenterDisplayName(row: Pick<CapabilityOverviewItem, "name" | "capability_id">): string {
  const name = String(row.name || "").trim();
  if (name) return name;
  const capId = String(row.capability_id || "").trim();
  return capId.split(":").filter(Boolean).pop() || capId || "Untitled";
}

export function capabilityCenterEnabledIds(state: CapabilityStateResult | null | undefined): Set<string> {
  const ids = new Set<string>();
  for (const item of state?.enabled_capabilities || []) {
    const value = String(item || "").trim();
    if (value) ids.add(value);
  }
  for (const item of state?.enabled || []) {
    const value = String(item?.capability_id || "").trim();
    if (value) ids.add(value);
  }
  return ids;
}

export function capabilityCenterHiddenIds(state: CapabilityStateResult | null | undefined): string[] {
  const out: string[] = [];
  for (const item of state?.actor_hidden_capabilities || []) {
    const value = String(item || "").trim();
    if (value && !out.includes(value)) out.push(value);
  }
  return out;
}

export function capabilityCenterIsBlocked(row: Pick<CapabilityOverviewItem, "blocked_global" | "policy_level" | "qualification_status">): boolean {
  const policyLevel = String(row.policy_level || "").trim().toLowerCase();
  const qualification = String(row.qualification_status || "").trim().toLowerCase();
  return Boolean(row.blocked_global) || policyLevel === "blocked" || qualification === "blocked";
}

export function capabilityCenterNeedsSetup(row: Pick<CapabilityOverviewItem, "readiness_preview" | "enable_supported" | "cached_install_state">): boolean {
  const preview = row.readiness_preview;
  const status = String(preview?.preview_status || "").trim().toLowerCase();
  const cachedInstallState = String(row.cached_install_state || preview?.cached_install_state || "").trim().toLowerCase();
  if (["missing_env", "needs_env", "needs_setup", "install_failed", "failed", "unavailable"].includes(status)) return true;
  if (["failed", "missing_env", "needs_setup"].includes(cachedInstallState)) return true;
  return row.enable_supported === false && status !== "already_active";
}

export function capabilityCenterMatchesQuery(row: CapabilityOverviewItem, query: string): boolean {
  const needle = String(query || "").trim().toLowerCase();
  if (!needle) return true;
  const haystack = [
    row.capability_id,
    row.name,
    row.description_short,
    row.source_id,
    row.source_uri,
    row.policy_level,
    row.qualification_status,
    ...(row.tags || []),
    ...(row.tool_names || []),
  ].map((value) => String(value || "").toLowerCase()).join("\n");
  return haystack.includes(needle);
}

export function filterCapabilityCenterRemovedItems(
  items: CapabilityOverviewItem[],
  removedIds: Set<string>,
): CapabilityOverviewItem[] {
  if (!removedIds.size) return items;
  return items.filter((item) => !removedIds.has(String(item.capability_id || "").trim()));
}

export function capabilityCenterIsReadonlySystem(row: CapabilityOverviewItem, state?: CapabilityStateResult | null): boolean {
  const sourceId = String(row.source_id || "").trim();
  if (sourceId !== "cccc_builtin") return false;
  const capId = String(row.capability_id || "").trim();
  const enabled = capabilityCenterEnabledIds(state).has(capId);
  return capabilityCenterRemovalAction(row, { enabled }) === "none";
}

export function capabilityCenterIsSlashVisible(
  row: CapabilityOverviewItem,
  state?: CapabilityStateResult | null,
): boolean {
  const capId = String(row.capability_id || "").trim();
  return Boolean(
    capId
      && capabilityCenterEnabledIds(state).has(capId)
      && canManageSlashCommandVisibility(row)
      && !isCapabilityHiddenFromSlashCommands(capId, capabilityCenterHiddenIds(state)),
  );
}

export function capabilityCenterIsSlashHidden(
  row: CapabilityOverviewItem,
  state?: CapabilityStateResult | null,
): boolean {
  const capId = String(row.capability_id || "").trim();
  return Boolean(
    capId
      && capabilityCenterEnabledIds(state).has(capId)
      && canManageSlashCommandVisibility(row)
      && isCapabilityHiddenFromSlashCommands(capId, capabilityCenterHiddenIds(state)),
  );
}

export function capabilityCenterFilterItemsForSystemVisibility(
  items: CapabilityOverviewItem[],
  input: {
    showSystem: boolean;
    query?: string;
    state?: CapabilityStateResult | null;
  },
): CapabilityOverviewItem[] {
  const query = String(input.query || "").trim();
  if (input.showSystem) return items;
  if (query) return items.filter((row) => capabilityCenterMatchesQuery(row, query));
  return items.filter((row) => !capabilityCenterIsReadonlySystem(row, input.state));
}

export function filterCapabilityCenterItems(
  items: CapabilityOverviewItem[],
  input: {
    query?: string;
    typeFilter?: CapabilityCenterTypeFilter;
    stateFilter?: CapabilityCenterStateFilter;
    state?: CapabilityStateResult | null;
  },
): CapabilityOverviewItem[] {
  const typeFilter = input.typeFilter || "all";
  const stateFilter = input.stateFilter || "all";
  const enabledIds = capabilityCenterEnabledIds(input.state);

  return items.filter((row) => {
    const type = capabilityCenterType(row);
    if (typeFilter !== "all" && type !== typeFilter) return false;
    if (!capabilityCenterMatchesQuery(row, input.query || "")) return false;

    const capId = String(row.capability_id || "").trim();
    if (stateFilter === "enabled") return enabledIds.has(capId);
    if (stateFilter === "slash_visible") return capabilityCenterIsSlashVisible(row, input.state);
    if (stateFilter === "slash_hidden") return capabilityCenterIsSlashHidden(row, input.state);
    if (stateFilter === "blocked") return capabilityCenterIsBlocked(row);
    if (stateFilter === "needs_setup") return capabilityCenterNeedsSetup(row);
    return true;
  });
}

export function mergeCapabilityCenterStickyItems(
  items: CapabilityOverviewItem[],
  stickyItems: CapabilityOverviewItem[],
): CapabilityOverviewItem[] {
  if (!stickyItems.length) return items;
  const seenIds = new Set(items.map((row) => String(row.capability_id || "").trim()).filter(Boolean));
  const merged = [...items];
  for (const row of stickyItems) {
    const capId = String(row.capability_id || "").trim();
    if (!capId || seenIds.has(capId)) continue;
    merged.push(row);
    seenIds.add(capId);
  }
  return merged;
}

export function summarizeCapabilityCenter(items: CapabilityOverviewItem[], state?: CapabilityStateResult | null): CapabilityCenterStats {
  const enabledIds = capabilityCenterEnabledIds(state);
  const sources = new Set<string>();
  const stats: CapabilityCenterStats = {
    total: items.length,
    skills: 0,
    mcp: 0,
    packs: 0,
    enabled: 0,
    slashHidden: 0,
    blocked: 0,
    needsSetup: 0,
    sources: 0,
  };
  for (const row of items) {
    const type = capabilityCenterType(row);
    if (type === "skill") stats.skills += 1;
    if (type === "mcp") stats.mcp += 1;
    if (type === "pack") stats.packs += 1;
    const capId = String(row.capability_id || "").trim();
    if (enabledIds.has(capId)) stats.enabled += 1;
    if (capabilityCenterIsSlashHidden(row, state)) stats.slashHidden += 1;
    if (capabilityCenterIsBlocked(row)) stats.blocked += 1;
    if (capabilityCenterNeedsSetup(row)) stats.needsSetup += 1;
    const sourceId = String(row.source_id || "").trim();
    if (sourceId) sources.add(sourceId);
  }
  stats.sources = sources.size;
  return stats;
}
