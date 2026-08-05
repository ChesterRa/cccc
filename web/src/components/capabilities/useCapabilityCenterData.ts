import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "../../services/api";
import type {
  CapabilityOverviewItem,
  CapabilitySourceInstance,
  CapabilitySourceState,
  CapabilityStateResult,
} from "../../types";
import {
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
  pageIndex: number;
  pageSize: number;
  query: string;
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
    pageIndex,
    pageSize,
    query,
    stateFilter,
    typeFilter,
  } = options;
  const [items, setItems] = useState<CapabilityOverviewItem[]>([]);
  const [sources, setSources] = useState<Record<string, CapabilitySourceState>>({});
  const [sourceInstances, setSourceInstances] = useState<CapabilitySourceInstance[]>([]);
  const [state, setState] = useState<CapabilityStateResult | null>(null);
  const [summaryStats, setSummaryStats] = useState<CapabilityCenterStats | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const [totalCount, setTotalCount] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
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
        groupId,
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
      setTotalCount(Number(response.result.total_count || nextItems.length) || 0);
      setHasMore(Boolean(response.result.has_more));
      setSources(response.result.sources || {});
      setSourceInstances(response.result.source_instances || []);
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
    pageIndex,
    pageSize,
    query,
    stateFilter,
    typeFilter,
  ]);

  const loadState = useCallback(async () => {
    const sequence = ++stateRequestSequence.current;
    if (!groupId) {
      setState(null);
      return;
    }
    const response = await api.fetchGroupCapabilityState(groupId, "user", { noCache: true });
    if (sequence !== stateRequestSequence.current) return;
    if (response.ok) {
      setState(response.result);
    } else {
      setError(response.error?.message || failedStateMessage);
    }
  }, [failedStateMessage, groupId]);

  const refresh = useCallback(async () => {
    await Promise.all([loadOverview(), loadState()]);
  }, [loadOverview, loadState]);

  useEffect(() => {
    if (isOpen) void loadOverview();
  }, [isOpen, loadOverview]);

  useEffect(() => {
    if (isOpen) void loadState();
  }, [isOpen, loadState]);

  return {
    error,
    hasMore,
    items,
    loading,
    refresh,
    selectedId,
    setError,
    setSelectedId,
    setState,
    sourceInstances,
    sources,
    state,
    summaryStats,
    totalCount,
  };
}
