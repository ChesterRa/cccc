import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import * as api from "../services/api";
import { buildSlashCommands, type SlashCommandItem } from "../utils/slashCommands";
import { subscribeCapabilityChanged } from "../utils/capabilityEvents";

const slashCommandCache = new Map<string, SlashCommandItem[]>();

function cachedSlashCommands(groupId: string): SlashCommandItem[] {
  return slashCommandCache.get(groupId) || buildSlashCommands({ state: null });
}

function cacheSlashCommands(groupId: string, commands: SlashCommandItem[]): SlashCommandItem[] {
  slashCommandCache.set(groupId, commands);
  return commands;
}

export function useSlashCommandState(selectedGroupId: string) {
  const selectedGid = useMemo(() => String(selectedGroupId || "").trim(), [selectedGroupId]);
  const selectedGidRef = useRef(selectedGid);
  const requestVersionRef = useRef(0);
  selectedGidRef.current = selectedGid;
  const [snapshot, setSnapshot] = useState<{ groupId: string; commands: SlashCommandItem[] }>(
    () => {
      return {
        groupId: selectedGid,
        commands: selectedGid ? cachedSlashCommands(selectedGid) : [],
      };
    },
  );

  const loadSlashCommands = useCallback(
    async (noCache: boolean) => {
      const gid = selectedGid;
      if (!gid) return;
      const requestVersion = ++requestVersionRef.current;
      try {
        const stateResp = await api.fetchSlashCommandCapabilityState(gid, "user", { noCache });
        if (
          !stateResp.ok ||
          selectedGidRef.current !== gid ||
          requestVersionRef.current !== requestVersion
        ) {
          return;
        }
        const commands = cacheSlashCommands(gid, buildSlashCommands({ state: stateResp.result }));
        setSnapshot({ groupId: gid, commands });
      } catch {
        // Keep the last known-good catalog. A later event or SSE reconnect retries it.
      }
    },
    [selectedGid],
  );

  const refreshSlashCommands = useCallback(() => loadSlashCommands(true), [loadSlashCommands]);

  const visibleSlashCommands = selectedGid
    ? snapshot.groupId === selectedGid
      ? snapshot.commands
      : cachedSlashCommands(selectedGid)
    : [];

  useEffect(() => {
    const gid = selectedGid;
    if (!gid) return;
    void loadSlashCommands(false);
    return () => {
      requestVersionRef.current += 1;
    };
  }, [loadSlashCommands, selectedGid]);

  useEffect(() => {
    return subscribeCapabilityChanged(selectedGid, () => {
      void refreshSlashCommands();
    });
  }, [refreshSlashCommands, selectedGid]);

  return { slashCommands: visibleSlashCommands, refreshSlashCommands };
}
