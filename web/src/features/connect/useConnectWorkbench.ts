import { useCallback, useEffect, useState } from "react";
import { apiJson } from "../../services/api/base";
import type { ConnectInstance, ConnectSnapshot, RemoteGroup } from "./protocol";

export type RemoteListing = {
  deviceId: string;
  origin: string | null;
  groups: RemoteGroup[];
  checkedAt: number;
};
export type RemoteSelection = {
  instanceId: string;
  groupId: string;
  epoch: number;
  revision: number;
  action?: "connections";
};

export function useConnectWorkbench(enabled: boolean, refreshEntryAccess: () => Promise<boolean>) {
  const [snapshot, setSnapshot] = useState<ConnectSnapshot | null>(null);
  const [listings, setListings] = useState<Record<string, RemoteListing>>({});
  const [selected, setSelected] = useState<RemoteSelection | null>(null);
  const [collapsedInstances, setCollapsedInstances] = useState<string[]>([]);
  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    const poll = async () => {
      if (!enabled) {
        setSnapshot(null);
        setListings({});
        setSelected(null);
        return;
      }
      const administrator = await refreshEntryAccess();
      if (cancelled) return;
      if (!administrator) {
        setSnapshot(null);
        setListings({});
        setSelected(null);
      } else {
        const result = await apiJson<{ connect: ConnectSnapshot | null }>("/api/v1/connect", {
          signal: AbortSignal.timeout(10000),
        });
        if (cancelled) return;
        const next = result.ok ? result.result.connect : null;
        setSnapshot(next);
        setListings((previous) =>
          Object.fromEntries(
            Object.entries(previous).filter(
              ([id, listing]) =>
                next?.directory &&
                Date.parse(next.directory.expires_at) > Date.now() &&
                next.directory.instances.some(
                  (entry) =>
                    entry.instance_id === id &&
                    entry.device_id === listing.deviceId &&
                    entry.public_origin === listing.origin,
                ),
            ),
          ),
        );
      }
      timer = window.setTimeout(() => void poll(), 15000);
    };
    void poll();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [enabled, refreshEntryAccess]);

  const directory =
    enabled && snapshot?.directory && Date.parse(snapshot.directory.expires_at) > Date.now()
      ? snapshot.directory
      : null;
  const instances =
    directory?.instances.filter((entry) => entry.instance_id !== snapshot?.instance_id) || [];
  const activeInstance = selected
    ? instances.find((entry) => entry.instance_id === selected.instanceId) || null
    : null;
  const select = useCallback((instanceId: string, groupId = "", action?: "connections") => {
    setCollapsedInstances((previous) => previous.filter((id) => id !== instanceId));
    setSelected((previous) => ({
      instanceId,
      groupId,
      action,
      epoch: previous?.instanceId === instanceId ? previous.epoch : (previous?.epoch || 0) + 1,
      revision: (previous?.revision || 0) + 1,
    }));
  }, []);
  // A target may report its initial/default selection after a newer sidebar
  // click. Only reports from the current navigation may update the entry.
  const reflectSelection = useCallback((instanceId: string, groupId: string, revision: number) => {
    setSelected((previous) =>
      previous?.instanceId === instanceId &&
      previous.revision === revision &&
      previous.groupId !== groupId
        ? { ...previous, groupId }
        : previous,
    );
  }, []);
  const remember = useCallback((instance: ConnectInstance, groups: RemoteGroup[] | null) => {
    setListings((previous) => {
      const next = { ...previous };
      if (groups)
        next[instance.instance_id] = {
          deviceId: instance.device_id,
          origin: instance.public_origin,
          groups,
          checkedAt: Date.now(),
        };
      else delete next[instance.instance_id];
      return next;
    });
  }, []);
  return {
    instances,
    listings: directory ? listings : {},
    collapsedInstances,
    toggleExpanded: (instanceId: string) =>
      setCollapsedInstances((previous) =>
        previous.includes(instanceId)
          ? previous.filter((id) => id !== instanceId)
          : [...previous, instanceId],
      ),
    selected: enabled ? selected : null,
    activeInstance,
    ownInstance: directory?.instances.find((entry) => entry.instance_id === snapshot?.instance_id),
    select,
    reflectSelection,
    selectLocal: () => {
      setSelected(null);
    },
    remember,
  };
}

export type ConnectWorkbench = ReturnType<typeof useConnectWorkbench>;

export function instanceListing(workbench: ConnectWorkbench, instance: ConnectInstance) {
  const listing = workbench.listings[instance.instance_id];
  return listing?.deviceId === instance.device_id && listing.origin === instance.public_origin
    ? listing
    : null;
}
