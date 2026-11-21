import { useEffect, useRef, useState } from "react";
import { useSessionNodeTicket } from "./useSessionNodeTicket";
import {
  fetchSessionNodes,
  revokeSessionNode,
  type SessionNodeDevice,
} from "../../services/api/sessionNodes";

export function useSessionNodes() {
  const [devices, setDevices] = useState<SessionNodeDevice[]>([]);
  const [supported, setSupported] = useState<boolean | null>(null);
  const [error, setError] = useState("");
  const [loadError, setLoadError] = useState("");
  const [busy, setBusy] = useState(false);
  const invitation = useSessionNodeTicket(setError);
  const [revokeId, setRevokeId] = useState("");
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const load = async () => {
      try {
        const response = await fetchSessionNodes();
        if (cancelled) return;
        if (response.ok) {
          setLoadError("");
          setSupported(response.result.supported);
          setDevices(response.result.devices);
        } else if (response.error.httpStatus === 404) setSupported(false);
        else setLoadError(response.error.message);
      } catch (error) {
        if (!cancelled) setLoadError(String(error));
      }
      if (!cancelled) timer = setTimeout(() => void load(), 5000);
    };
    void load();
    return () => {
      cancelled = true;
      alive.current = false;
      clearTimeout(timer);
    };
  }, []);

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
    supported,
    error: error || loadError,
    busy: busy || invitation.creating || invitation.copying,
    revokeId,
    setRevokeId,
    revoke,
  };
}
