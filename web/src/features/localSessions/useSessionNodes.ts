import { useCallback, useEffect, useRef, useState } from "react";
import type { LocalSessionSource } from "../../types";
import {
  fetchSessionNodes,
  revokeSessionNode,
  setSessionNodeRelay,
  stopSessionNodeGateway,
  type SessionNodeDevice,
  type SessionNodeEndpoint,
  type SessionNodeRelayStatus,
} from "../../services/api/sessionNodes";

type ReadyDirectory = {
  status: "ready";
  devices: SessionNodeDevice[];
  endpoint: SessionNodeEndpoint | null;
  relay: SessionNodeRelayStatus;
  supportedRuntimes: LocalSessionSource["runtime"][];
};

type Directory =
  | ReadyDirectory
  | { status: "loading" | "rechecking" | "unsupported" | "forbidden" | "failed" };

export type RelayToggleError = "" | "unavailable" | "upgrade" | "failed";

const DEFAULT_RELAY: SessionNodeRelayStatus = {
  enabled: false,
  effective: false,
  public_url: null,
};

export function useSessionNodes(active = true) {
  const [directory, setDirectory] = useState<Directory>({ status: "loading" });
  const [revokeId, setRevokeId] = useState("");
  const [revoking, setRevoking] = useState(false);
  const [revokeError, setRevokeError] = useState(false);
  const [relayBusy, setRelayBusy] = useState(false);
  const [relayError, setRelayError] = useState<RelayToggleError>("");
  const [stopping, setStopping] = useState(false);
  const [stopError, setStopError] = useState(false);
  const alive = useRef(false);
  const mutation = useRef(0);
  const inFlight = useRef(false);
  const forbid = useCallback(() => {
    mutation.current++;
    setRevokeId("");
    setRevokeError(false);
    setRelayError("");
    setStopError(false);
    setDirectory({ status: "forbidden" });
  }, []);
  useEffect(() => {
    if (!active) {
      // Preserve an authorized ticket while the device tabs are hidden, but verify access again before showing it.
      setDirectory((current) =>
        current.status === "ready" || current.status === "failed"
          ? { status: "rechecking" }
          : current,
      );
      return;
    }
    alive.current = true;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    const load = async () => {
      const version = mutation.current;
      try {
        const response = await fetchSessionNodes(controller.signal);
        if (controller.signal.aborted) return;
        if (!response.ok) {
          const status = response.error.httpStatus;
          if (status === 401 || status === 403) forbid();
          else setDirectory({ status: status === 404 ? "unsupported" : "failed" });
        } else if (version === mutation.current) {
          const result = response.result;
          if (result?.supported === false) setDirectory({ status: "unsupported" });
          else if (result?.supported === true && Array.isArray(result.devices))
            setDirectory({
              status: "ready",
              devices: result.devices,
              endpoint: result.endpoint ?? null,
              relay: result.relay ?? DEFAULT_RELAY,
              supportedRuntimes: Array.isArray(result.supported_runtimes)
                ? result.supported_runtimes
                : [],
            });
          else setDirectory({ status: "failed" });
        }
      } catch {
        if (!controller.signal.aborted) setDirectory({ status: "failed" });
      }
      if (!controller.signal.aborted) timer = setTimeout(() => void load(), 5000);
    };
    void load();
    return () => {
      alive.current = false;
      controller.abort();
      clearTimeout(timer);
    };
  }, [active, forbid]);

  async function revoke() {
    if (
      inFlight.current ||
      directory.status !== "ready" ||
      !directory.devices.some((d) => d.node_id === revokeId && d.enabled)
    )
      return;
    inFlight.current = true;
    setRevoking(true);
    setRevokeError(false);
    try {
      const response = await revokeSessionNode(revokeId);
      if (!alive.current) return;
      if (response.ok && response.result.revoked === true) {
        mutation.current++;
        setDirectory((previous) =>
          previous.status === "ready"
            ? {
                ...previous,
                devices: previous.devices.map((device) =>
                  device.node_id === revokeId
                    ? { ...device, enabled: false, connected: false }
                    : device,
                ),
              }
            : previous,
        );
        setRevokeId("");
      } else if (
        !response.ok &&
        (response.error.httpStatus === 401 || response.error.httpStatus === 403)
      )
        forbid();
      else setRevokeError(true);
    } catch {
      if (alive.current) setRevokeError(true);
    } finally {
      inFlight.current = false;
      if (alive.current) setRevoking(false);
    }
  }

  async function toggleRelay(enabled: boolean): Promise<boolean> {
    if (inFlight.current || directory.status !== "ready") return false;
    inFlight.current = true;
    setRelayBusy(true);
    setRelayError("");
    try {
      const response = await setSessionNodeRelay(enabled);
      if (!alive.current) return false;
      if (response.ok && response.result?.relay) {
        mutation.current++;
        const nextRelay = response.result.relay;
        setDirectory((previous) =>
          previous.status === "ready"
            ? {
                ...previous,
                relay: nextRelay,
                endpoint: previous.endpoint
                  ? {
                      ...previous.endpoint,
                      relay_url: nextRelay.effective ? nextRelay.public_url : null,
                    }
                  : null,
              }
            : previous,
        );
        return true;
      }
      if (!response.ok) {
        const status = response.error.httpStatus;
        if (status === 401 || status === 403) forbid();
        else if (status === 404 || response.error.code === "session_node_upgrade_required")
          setRelayError("upgrade");
        else if (response.error.code === "session_node_relay_unavailable")
          setRelayError("unavailable");
        else setRelayError("failed");
      } else setRelayError("failed");
      return false;
    } catch {
      if (alive.current) setRelayError("failed");
      return false;
    } finally {
      inFlight.current = false;
      if (alive.current) setRelayBusy(false);
    }
  }

  async function stopGateway(): Promise<boolean> {
    if (inFlight.current || directory.status !== "ready") return false;
    inFlight.current = true;
    setStopping(true);
    setStopError(false);
    try {
      const response = await stopSessionNodeGateway();
      if (!alive.current) return false;
      if (response.ok && response.result?.stopped === true) {
        mutation.current++;
        setDirectory((previous) =>
          previous.status === "ready"
            ? {
                ...previous,
                endpoint: null,
                relay: { ...previous.relay, effective: false },
                devices: previous.devices.map((device) => ({ ...device, connected: false })),
              }
            : previous,
        );
        return true;
      }
      if (!response.ok && (response.error.httpStatus === 401 || response.error.httpStatus === 403))
        forbid();
      else setStopError(true);
      return false;
    } catch {
      if (alive.current) setStopError(true);
      return false;
    } finally {
      inFlight.current = false;
      if (alive.current) setStopping(false);
    }
  }

  return {
    directory,
    revokeId,
    setRevokeId,
    revoking,
    revokeError,
    revoke,
    relayBusy,
    relayError,
    toggleRelay,
    stopping,
    stopError,
    stopGateway,
    forbid,
  };
}
