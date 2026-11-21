import { useCallback, useEffect, useRef, useState } from "react";
import { useSessionNodeTicket } from "./useSessionNodeTicket";
import {
  fetchPendingSessionNodeJoins,
  fetchSessionNodes,
  revokeSessionNode,
  type SessionNodeDevice,
  type SessionNodeJoinRequest,
} from "../../services/api/sessionNodes";

export interface SessionNodeJoinDirectory {
  url: string | null;
  pending: Pick<SessionNodeJoinRequest, "request_id" | "name" | "user_code">[] | null;
  error: "forbidden" | "failed" | null;
}

export function useSessionNodes(includeJoinRequests = true) {
  const [directoryStatus, setDirectoryStatus] = useState<
    "loading" | "ready" | "forbidden" | "failed"
  >("loading");
  const [devices, setDevices] = useState<SessionNodeDevice[]>([]);
  const [supported, setSupported] = useState<boolean | null>(null);
  const [join, setJoin] = useState<SessionNodeJoinDirectory | null>(null);
  const [error, setError] = useState("");
  const [loadError, setLoadError] = useState("");
  const [busy, setBusy] = useState(false);
  const invitation = useSessionNodeTicket(setError);
  const [revokeId, setRevokeId] = useState("");
  const alive = useRef(true);
  const [revision, setRevision] = useState(0);
  const refreshDirectory = useCallback(() => {
    // A previously connected key may be requesting a fresh grant. Verify it after approval.
    setDevices([]);
    setDirectoryStatus("loading");
    setRevision((value) => value + 1);
  }, []);
  useEffect(() => {
    alive.current = true;
    let cancelled = false;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    const load = async () => {
      try {
        const response = await fetchSessionNodes(controller.signal);
        if (cancelled) return;
        if (response.ok) {
          setDirectoryStatus("ready");
          setLoadError("");
          setSupported(response.result.supported);
          setDevices(response.result.devices);
          if (
            includeJoinRequests &&
            response.result.join_supported === true &&
            response.result.supported
          ) {
            const url = response.result.join_url ?? null;
            setJoin((previous) =>
              previous?.url === url ? previous : { url, pending: null, error: null },
            );
            const pending = await fetchPendingSessionNodeJoins(controller.signal).catch(() => null);
            if (cancelled) return;
            setJoin(
              pending?.ok && Array.isArray(pending.result.requests)
                ? {
                    url,
                    pending: pending.result.requests.map(({ request_id, name, user_code }) => ({
                      request_id,
                      name,
                      user_code,
                    })),
                    error: null,
                  }
                : {
                    url,
                    pending: null,
                    error:
                      pending &&
                      !pending.ok &&
                      (pending.error.httpStatus === 403 || pending.error.httpStatus === 401)
                        ? "forbidden"
                        : "failed",
                  },
            );
          } else setJoin(null);
        } else {
          setJoin(null);
          setDevices([]);
          setDirectoryStatus(
            response.error.httpStatus === 401 || response.error.httpStatus === 403
              ? "forbidden"
              : "failed",
          );
          if (response.error.httpStatus === 404) {
            setSupported(false);
            setLoadError("");
          } else setLoadError(response.error.message);
        }
      } catch (error) {
        if (!cancelled) {
          setDevices([]);
          setDirectoryStatus("failed");
          setLoadError(String(error));
        }
      }
      if (!cancelled) timer = setTimeout(() => void load(), 5000);
    };
    void load();
    return () => {
      cancelled = true;
      alive.current = false;
      controller.abort();
      clearTimeout(timer);
    };
  }, [includeJoinRequests, revision]);

  const revoke = async () => {
    setBusy(true);
    setError("");
    try {
      const response = await revokeSessionNode(revokeId);
      if (!alive.current) return;
      if (response.ok) {
        setDevices((previous) =>
          previous.map((device) =>
            device.node_id === revokeId ? { ...device, enabled: false, connected: false } : device,
          ),
        );
        setRevokeId("");
      } else setError(response.error.message);
    } catch (error) {
      if (alive.current) setError(String(error));
    } finally {
      if (alive.current) setBusy(false);
    }
  };
  return {
    ...invitation,
    devices,
    directoryStatus,
    refreshDirectory,
    supported,
    join,
    error: error || loadError,
    busy: busy || invitation.creating || invitation.copying,
    revokeId,
    setRevokeId,
    revoke,
  };
}
