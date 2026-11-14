import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "../../services/api";
import type {
  CapabilityDomain,
  CapabilityOverviewItem,
  CapabilitySourceState,
  CapabilityStateResult,
} from "../../types";
import {
  capabilityCenterStateReadable,
  normalizeCapabilityCenterPagination,
  summarizeCapabilityCenter,
  type CapabilityCenterStateFilter,
  type CapabilityCenterStats,
  type CapabilityCenterTypeFilter,
} from "./capabilityCenterModel";

const CLIENT_FILTER_LIMIT = 2000;

interface CapabilityCenterDataOptions {
  clientPagination: boolean;
  failedLoadMessage: string;
  failedStateMessage: string;
  groupId: string;
  isOpen: boolean;
  /** The language release recommendations are read in. */
  locale: string;
  pageIndex: number;
  pageSize: number;
  query: string;
  /** Lists one catalog source only, such as the release recommendations. */
  sourceId?: string;
  stateFilter: CapabilityCenterStateFilter;
  typeFilter: CapabilityCenterTypeFilter;
}

export function useCapabilityCenterData(options: CapabilityCenterDataOptions) {
  const {
    clientPagination,
    failedLoadMessage,
    failedStateMessage,
    groupId,
    isOpen,
    locale,
    pageIndex,
    pageSize,
    query,
    sourceId,
    stateFilter,
    typeFilter,
  } = options;
  const [items, setItems] = useState<CapabilityOverviewItem[]>([]);
  const [matchedQuery, setMatchedQuery] = useState<string | null>(null);
  const [domains, setDomains] = useState<CapabilityDomain[]>([]);
  const [sources, setSources] = useState<Record<string, CapabilitySourceState>>({});
  const [state, setState] = useState<CapabilityStateResult | null>(null);
  const [summaryStats, setSummaryStats] = useState<CapabilityCenterStats | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const [totalCount, setTotalCount] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  // A failed state read is kept apart from list and action errors: reloading the list
  // must not clear it, only a later state read that succeeds.
  const [stateError, setStateError] = useState("");
  const requestSequence = useRef(0);
  const stateRequestSequence = useRef(0);

  const loadOverview = useCallback(async () => {
    const sequence = ++requestSequence.current;
    const normalizedPageSize = normalizeCapabilityCenterPagination({
      pageIndex: 0,
      pageSize,
      totalCount: 0,
    }).pageSize;
    const offset = clientPagination
      ? 0
      : Math.max(0, Math.trunc(Number(pageIndex) || 0)) * normalizedPageSize;
    const limit = clientPagination ? CLIENT_FILTER_LIMIT : normalizedPageSize;
    setLoading(true);
    setError("");
    try {
      const response = await api.fetchCapabilityOverview({
        includeIndexed: true,
        limit,
        offset,
        query: query || undefined,
        kind: typeFilter,
        policy: stateFilter === "blocked" ? "blocked" : "all",
        sourceId,
        groupId,
        locale,
      });
      if (sequence !== requestSequence.current) return;
      if (!response.ok) {
        setError(response.error?.message || failedLoadMessage);
        return;
      }
      const nextItems = response.result.items || [];
      const nextStats = summarizeCapabilityCenter(nextItems, null);
      const kindCounts = response.result.kind_counts || {};
      setItems(nextItems);
      setMatchedQuery(query);
      setDomains(Array.isArray(response.result.domains) ? response.result.domains : []);
      setTotalCount(Number(response.result.total_count || nextItems.length) || 0);
      setHasMore(Boolean(response.result.has_more));
      setSources(response.result.sources || {});
      setSummaryStats({
        total: Number(response.result.total_count || nextItems.length) || 0,
        skills: Number(kindCounts.skill || 0),
        mcp: Number(kindCounts.mcp || 0),
        packs: Number(kindCounts.pack || 0),
        enabled: 0,
        slashHidden: nextStats.slashHidden,
        blocked: nextStats.blocked,
        needsSetup: nextStats.needsSetup,
        sources: Object.keys(response.result.sources || {}).length,
      });
      setSelectedId((current) =>
        current && nextItems.some((item) => item.capability_id === current)
          ? current
          : String(nextItems[0]?.capability_id || ""),
      );
    } finally {
      if (sequence === requestSequence.current) setLoading(false);
    }
  }, [
    clientPagination,
    failedLoadMessage,
    groupId,
    locale,
    pageIndex,
    pageSize,
    query,
    sourceId,
    stateFilter,
    typeFilter,
  ]);

  const loadState = useCallback(async (): Promise<CapabilityStateResult | null> => {
    const sequence = ++stateRequestSequence.current;
    if (!groupId) {
      setState(null);
      setStateError("");
      return null;
    }
    const response = await api.fetchGroupCapabilityState(groupId, "user", { noCache: true });
    if (sequence !== stateRequestSequence.current) return null;
    // A read the center cannot show keeps the last state on screen and reports the failure.
    if (response.ok && capabilityCenterStateReadable(response.result)) {
      setState(response.result);
      setStateError("");
      return response.result;
    }
    setStateError((!response.ok && response.error?.message) || failedStateMessage);
    return null;
  }, [failedStateMessage, groupId]);

  // Resolves to the state it loaded, or null when that load failed or a newer one replaced it.
  const refresh = useCallback(async () => {
    const [, nextState] = await Promise.all([loadOverview(), loadState()]);
    return nextState;
  }, [loadOverview, loadState]);

  useEffect(() => {
    if (isOpen) void loadOverview();
  }, [isOpen, loadOverview]);

  useEffect(() => {
    if (isOpen) void loadState();
  }, [isOpen, loadState]);

  return {
    domains,
    error: error || stateError,
    hasMore,
    items,
    matchedQuery,
    loading,
    loadState,
    refresh,
    selectedId,
    setError,
    setSelectedId,
    sources,
    state,
    summaryStats,
    totalCount,
  };
}
