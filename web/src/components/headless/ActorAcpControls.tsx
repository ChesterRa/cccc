import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiJson } from "../../services/api/base";
import type { AcpPermission } from "../../services/api/codexVoice";
import { Button } from "../ui/button";
import { AcpPermissionList } from "./AcpPermissionList";

type ControlState = { generation: string; permissions: AcpPermission[]; working: boolean };
export function ActorAcpControls({
  groupId,
  actorId,
  visible,
  readOnly,
}: {
  groupId: string;
  actorId: string;
  visible: boolean;
  readOnly?: boolean;
}) {
  const { t } = useTranslation("actors");
  const [state, setState] = useState<ControlState | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const url = `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/headless/control`;
  useEffect(() => {
    setState(null);
    setError("");
    if (!visible || readOnly) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const abort = new AbortController();
    const poll = async () => {
      try {
        const response = await apiJson<{ state: ControlState | null }>(url, {
          signal: abort.signal,
        });
        if (alive && response.ok) setState(response.result.state);
      } catch {
        /* Unmount aborts the read; writes report their own failures. */
      }
      if (alive) timer = setTimeout(() => void poll(), 1200);
    };
    void poll();
    return () => {
      alive = false;
      clearTimeout(timer);
      abort.abort();
    };
  }, [url, visible, readOnly]);
  async function control(command: Record<string, unknown>) {
    if (!state || busy) return;
    setBusy(true);
    setError("");
    try {
      const response = await apiJson(url, {
        method: "POST",
        body: JSON.stringify({ ...command, generation: state.generation }),
      });
      if (!response.ok) setError(response.error.message);
    } catch {
      setError(t("acpControls.error"));
    } finally {
      setBusy(false);
    }
  }
  if (readOnly || !state) return null;
  return (
    <div className="flex-none space-y-2 border-t border-[var(--glass-border-subtle)] p-3">
      <AcpPermissionList
        key={state.generation}
        permissions={state.permissions}
        onInteract={(request_id, response) =>
          void control({ action: "interaction", request_id, response })
        }
        disabled={busy}
        onRespond={(request_id, allow) => void control({ action: "permission", request_id, allow })}
      />
      {state.working && (
        <Button
          size="sm"
          variant="secondary"
          disabled={busy}
          onClick={() => void control({ action: "cancel" })}
        >
          {t("acpControls.cancelCurrent")}
        </Button>
      )}
      {error && (
        <p role="alert" className="text-xs text-rose-600 dark:text-rose-400">
          {error}
        </p>
      )}
    </div>
  );
}
