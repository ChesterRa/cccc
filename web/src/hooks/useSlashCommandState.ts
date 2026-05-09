import { useCallback, useEffect, useMemo, useState } from "react";

import * as api from "../services/api";
import {
  buildSlashCommands,
  type SlashCommandItem,
} from "../utils/slashCommands";
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
  const [slashCommands, setSlashCommands] = useState<SlashCommandItem[]>(() => {
    return selectedGid ? cachedSlashCommands(selectedGid) : [];
  });

  const refreshSlashCommands = useCallback(async () => {
    const gid = selectedGid;
    if (!gid) return;
    try {
      const stateResp = await api.fetchGroupCapabilityState(gid, "user", { noCache: true });
      setSlashCommands(cacheSlashCommands(gid, buildSlashCommands({
        state: stateResp.ok ? stateResp.result : null,
      })));
    } catch {
      setSlashCommands(cachedSlashCommands(gid));
    }
  }, [selectedGid]);

  const visibleSlashCommands = selectedGid ? slashCommands : [];

  useEffect(() => {
    let cancelled = false;
    const gid = selectedGid;
    if (!gid) return;
    queueMicrotask(() => {
      if (!cancelled) setSlashCommands(cachedSlashCommands(gid));
    });
    void api.fetchGroupCapabilityState(gid, "user").then((stateResp) => {
      if (cancelled) return;
      setSlashCommands(cacheSlashCommands(gid, buildSlashCommands({
        state: stateResp.ok ? stateResp.result : null,
      })));
    }).catch(() => {
      if (!cancelled) setSlashCommands(cachedSlashCommands(gid));
    });
    return () => {
      cancelled = true;
    };
  }, [selectedGid]);

  useEffect(() => {
    const handleFocus = () => {
      void refreshSlashCommands();
    };
    window.addEventListener("focus", handleFocus);
    return () => {
      window.removeEventListener("focus", handleFocus);
    };
  }, [refreshSlashCommands]);

  useEffect(() => {
    return subscribeCapabilityChanged(selectedGid, () => {
      void refreshSlashCommands();
    });
  }, [refreshSlashCommands, selectedGid]);

  return { slashCommands: visibleSlashCommands, refreshSlashCommands };
}
