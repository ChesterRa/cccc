import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { copyTextToClipboard } from "../../utils/copy";
import {
  createSessionNodeInvitation,
  fetchSessionNodes,
  revokeSessionNode,
  type SessionNodeDevice,
} from "../../services/api/sessionNodes";

export function useSessionNodes() {
  const { t } = useTranslation("layout");
  const [devices, setDevices] = useState<SessionNodeDevice[]>([]);
  const [supported, setSupported] = useState<boolean | null>(null);
  const [error, setError] = useState("");
  const [loadError, setLoadError] = useState("");
  const [busy, setBusy] = useState(false);
  const [ticket, setTicket] = useState<{ ticket: string; expires_at: string } | null>(null);
  const [copied, setCopied] = useState(false);
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

  const create = async () => {
    setBusy(true);
    setError("");
    try {
      const response = await createSessionNodeInvitation();
      if (!alive.current) return;
      if (response.ok) {
        setTicket(response.result);
        setCopied(false);
      } else setError(response.error.message);
    } catch (error) {
      if (alive.current) setError(String(error));
    } finally {
      if (alive.current) setBusy(false);
    }
  };
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
  const copy = async () => {
    if (!ticket) return;
    const success = await copyTextToClipboard(ticket.ticket);
    if (alive.current) {
      setCopied(success);
      if (!success) setError(t("common:copyFailed"));
    }
  };
  return {
    devices,
    supported,
    error: error || loadError,
    busy,
    ticket,
    copied,
    revokeId,
    setRevokeId,
    create,
    revoke,
    copy,
  };
}
