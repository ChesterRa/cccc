import { useEffect, useState } from "react";
import { apiJson } from "../services/api/base";

export type ConnectMentionGroup = {
  instance_id: string;
  instance_name: string;
  group_id: string;
  title: string;
  actors: { id: string; title?: string | null; enabled: boolean }[];
  fresh: boolean;
};

type Instance = { instance_id: string; display_name: string };
type Directory = {
  instances: Instance[];
  external_groups: { instance: Instance; group_id: string; title: string }[];
  account_error?: string | null;
  status?: string;
};
type Catalog = {
  instance: Instance;
  catalog: { groups: Pick<ConnectMentionGroup, "group_id" | "title" | "actors">[] } | null;
  fresh: boolean;
  next: string | null;
};

const EMPTY_GROUPS: ConnectMentionGroup[] = [];

function readCatalog<T>(groupId: string, query: Record<string, string>, signal: AbortSignal) {
  const search = new URLSearchParams(query);
  return apiJson<T>(`/api/v1/groups/${encodeURIComponent(groupId)}/connect/catalog?${search}`, {
    signal: AbortSignal.any([signal, AbortSignal.timeout(10000)]),
  });
}

// Read only the daemon's cached, authorized directory. Opening the helper never
// refreshes peers, opens their Web views, or starts a runtime.
export async function loadConnectMentionGroups(groupId: string, signal: AbortSignal) {
  const directory = await readCatalog<Directory>(groupId, {}, signal);
  if (!directory.ok) {
    // Restricted Web sessions deliberately retain the local-only helper.
    return { groups: [], incomplete: directory.error.code !== "admin_required" };
  }
  const groups = new Map<string, ConnectMentionGroup>();
  let incomplete =
    Boolean(directory.result.account_error) || directory.result.status === "unavailable";
  const peers = directory.result.instances.map((instance) => ({
    instance,
    group_id: "",
    title: "",
  }));
  peers.push(...directory.result.external_groups);
  // Bound concurrent local reads; process every catalog page, not just its first 20 Groups.
  for (let offset = 0; offset < peers.length && !signal.aborted; offset += 8) {
    await Promise.all(
      peers.slice(offset, offset + 8).map(async (peer) => {
        let after = "";
        do {
          const result = await readCatalog<Catalog>(
            groupId,
            {
              instance_id: peer.instance.instance_id,
              ...(peer.group_id ? { target_group_id: peer.group_id } : {}),
              ...(after ? { after } : {}),
              limit: "64",
            },
            signal,
          );
          if (!result.ok) {
            incomplete = true;
            return;
          }
          const { catalog, instance, fresh, next } = result.result;
          if (!catalog) {
            incomplete = true;
            // A currently authorized pair already names its target, even before
            // its first Actor catalog arrives. Do not invent any Actor entries.
            if (peer.group_id)
              groups.set(JSON.stringify([instance.instance_id, peer.group_id]), {
                instance_id: instance.instance_id,
                instance_name: instance.display_name,
                group_id: peer.group_id,
                title: peer.title,
                actors: [],
                fresh: false,
              });
          }
          for (const group of catalog?.groups || []) {
            groups.set(JSON.stringify([instance.instance_id, group.group_id]), {
              ...group,
              instance_id: instance.instance_id,
              instance_name: instance.display_name,
              fresh,
            });
          }
          if (!next) return;
          if (next <= after) {
            incomplete = true;
            return;
          }
          after = next;
        } while (!signal.aborted);
      }),
    );
  }
  return {
    groups: [...groups.values()].sort(
      (a, b) =>
        a.instance_name.localeCompare(b.instance_name) ||
        a.title.localeCompare(b.title) ||
        a.instance_id.localeCompare(b.instance_id) ||
        a.group_id.localeCompare(b.group_id),
    ),
    incomplete,
  };
}

export function useConnectMentionGroups(groupId: string, open: boolean) {
  const [state, setState] = useState<{
    groupId: string;
    groups: ConnectMentionGroup[];
    loading: boolean;
    incomplete: boolean;
  }>({ groupId: "", groups: [], loading: false, incomplete: false });
  useEffect(() => {
    if (!open || !groupId) return;
    const controller = new AbortController();
    setState({ groupId, groups: [], loading: true, incomplete: false });
    void loadConnectMentionGroups(groupId, controller.signal).then(
      (result) => {
        if (!controller.signal.aborted) setState({ groupId, ...result, loading: false });
      },
      () => {
        if (!controller.signal.aborted)
          setState({ groupId, groups: [], loading: false, incomplete: true });
      },
    );
    return () => controller.abort();
  }, [groupId, open]);
  return open && state.groupId === groupId
    ? state
    : { groups: EMPTY_GROUPS, loading: open, incomplete: false };
}
