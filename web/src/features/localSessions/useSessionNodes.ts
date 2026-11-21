import { useCallback, useEffect, useRef, useState } from "react";
import {
  fetchSessionNodes,
  revokeSessionNode,
  type SessionNodeDevice,
} from "../../services/api/sessionNodes";

type Directory =
  | { status: "ready"; devices: SessionNodeDevice[] }
  | { status: "loading" | "unsupported" | "forbidden" | "failed" };

export function useSessionNodes() {
  const [directory, setDirectory] = useState<Directory>({ status: "loading" });
  const [revokeId, setRevokeId] = useState("");
  const [revoking, setRevoking] = useState(false);
  const [revokeError, setRevokeError] = useState(false);
  const alive = useRef(false);
  const mutation = useRef(0);
  const inFlight = useRef(false);
  const forbid = useCallback(() => {
    mutation.current++;
    setRevokeId("");
    setRevokeError(false);
    setDirectory({ status: "forbidden" });
  }, []);
  useEffect(() => {
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
            setDirectory({ status: "ready", devices: result.devices });
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
  }, [forbid]);

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
  return { directory, revokeId, setRevokeId, revoking, revokeError, revoke, forbid };
}
