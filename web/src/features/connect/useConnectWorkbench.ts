import { requestWorkspaceNavigation } from "../../stores/workspaceNavigation";
import { useCallback, useEffect, useRef, useState } from "react";
import { apiJson } from "../../services/api/base";
import type {
  ConnectInstance,
  ConnectSnapshot,
  RemoteGroup,
  GroupConnectionSummary,
  ConnectStatusResponse,
} from "./protocol";

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

export function useConnectWorkbench(
  enabled: boolean,
  refreshEntryAccess: () => Promise<boolean | null>,
) {
  const binding = useRef("");
  const [lastDirectory, setLastDirectory] = useState<ConnectSnapshot["directory"]>(null);
  const [groupConnections, setGroupConnections] = useState<GroupConnectionSummary | null>(null);
  const [accountLabel, setAccountLabel] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [checkedAt, setCheckedAt] = useState(0);
  const [snapshot, setSnapshot] = useState<ConnectSnapshot | null>(null);
  const [listings, setListings] = useState<Record<string, RemoteListing>>({});
  const [selected, setSelected] = useState<RemoteSelection | null>(null);
  const [collapsedInstances, setCollapsedInstances] = useState<string[]>([]);
  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    const controller = new AbortController();
    const clear = () => {
      binding.current = "";
      setLastDirectory(null);
      setSnapshot(null);
      setListings({});
      setSelected(null);
      setAccountLabel(null);
      setGroupConnections(null);
    };
    const poll = async () => {
      if (!enabled) {
        clear();
        return;
      }
      const administrator = await refreshEntryAccess();
      if (cancelled) return;
      setCheckedAt(Date.now());
      setFailed(administrator === null);
      if (administrator === false) {
        clear();
      } else if (administrator === true) {
        const result = await apiJson<ConnectStatusResponse>("/api/v1/connect", {
          signal: AbortSignal.any([controller.signal, AbortSignal.timeout(10000)]),
        });
        if (cancelled) return;
        setFailed(!result.ok);
        if (!result.ok) {
          if (["unauthorized", "auth_required", "permission_denied"].includes(result.error.code))
            clear();
        } else {
          setAccountLabel(result.result.account_label || null);
          const next = result.result.connect;
          const nextBinding = next ? `${next.account_origin}/${next.device_id}` : "";
          const rejected =
            Boolean(next?.error_code) &&
            !next?.directory &&
            ![
              "membership_network",
              "membership_authorization_pending",
              "connect_directory_expired",
            ].includes(next?.error_code || "");
          const sameBinding = !rejected && Boolean(nextBinding) && binding.current === nextBinding;
          binding.current = nextBinding;
          if (!sameBinding) {
            setSelected(null);
            setLastDirectory(null);
          }
          if (next?.directory) setLastDirectory(next.directory);
          setGroupConnections(
            (previous) => result.result.group_connections || (sameBinding ? previous : null),
          );
          setSnapshot(next);
          setListings((previous) =>
            sameBinding && !next?.directory
              ? previous
              : Object.fromEntries(
                  Object.entries(sameBinding ? previous : {}).filter(
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
      }
      timer = window.setTimeout(() => void poll(), 15000);
    };
    void poll();
    return () => {
      cancelled = true;
      controller.abort();
      window.clearTimeout(timer);
    };
  }, [enabled, refreshEntryAccess]);

  const directory =
    enabled && snapshot?.directory && Date.parse(snapshot.directory.expires_at) > Date.now()
      ? snapshot.directory
      : null;
  const instances =
    (enabled ? lastDirectory : null)?.instances.filter(
      (entry) => entry.instance_id !== snapshot?.instance_id,
    ) || [];
  const activeInstance =
    directory && selected
      ? instances.find((entry) => entry.instance_id === selected.instanceId) || null
      : null;
  const selectionRef = useRef(selected);
  selectionRef.current = selected;
  const select = useCallback((instanceId: string, groupId = "", action?: "connections") => {
    const apply = () => {
      setCollapsedInstances((previous) => previous.filter((id) => id !== instanceId));
      setSelected((previous) => ({
        instanceId,
        groupId,
        action,
        epoch: previous?.instanceId === instanceId ? previous.epoch : (previous?.epoch || 0) + 1,
        revision: (previous?.revision || 0) + 1,
      }));
    };
    // The embedded editor guards Group changes in its own document. Switching
    // instances destroys that document, so its dirty flag is checked here.
    if (selectionRef.current?.instanceId === instanceId) apply();
    else requestWorkspaceNavigation(apply);
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
    groupConnections: enabled ? groupConnections : null,
    accountLabel: enabled ? accountLabel : null,
    failed,
    available: Boolean(directory),
    checkedAt,
    instances,
    listings: enabled ? listings : {},
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
