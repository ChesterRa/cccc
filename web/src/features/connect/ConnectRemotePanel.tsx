import { setWorkspaceDirty } from "../../stores/workspaceNavigation";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiJson } from "../../services/api/base";
import { Button } from "../../components/ui/button";
import { acceptsFrameMessage, CONNECT_CHANNEL, remoteGroups, type OpenFrame } from "./protocol";
import type { ConnectWorkbench } from "./useConnectWorkbench";

export function ConnectRemotePanel({
  workbench,
  onOpenSidebar,
}: {
  workbench: ConnectWorkbench;
  onOpenSidebar: () => void;
}) {
  const { t } = useTranslation("layout");
  const instance = workbench.activeInstance;
  const [attempt, setAttempt] = useState(0);
  const [opened, setOpened] = useState<OpenFrame | null>(null);
  const [error, setError] = useState("");
  const [ready, setReady] = useState(false);
  const iframe = useRef<HTMLIFrameElement>(null);
  const latest = useRef({ workbench, onOpenSidebar });
  latest.current = { workbench, onOpenSidebar };
  const instanceId = instance?.instance_id;
  const deviceId = instance?.device_id;
  const origin = instance?.public_origin;
  const epoch = workbench.selected?.epoch;
  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    let deadline: number | undefined;
    let readyDeadline: number | undefined;
    let current: OpenFrame | null = null;
    const dirtyOwner = Symbol("embedded workspace");
    let verifiedPeer = false;
    const frameId = crypto.randomUUID();
    const clear = (forget = true) => {
      setWorkspaceDirty(dirtyOwner, false);
      setOpened(null);
      setReady(false);
      const target = latest.current.workbench.activeInstance;
      if (forget && target) latest.current.workbench.remember(target, null);
    };
    const fail = (message: string) => {
      clear();
      setError(message);
      window.clearTimeout(timer);
      window.clearTimeout(deadline);
      window.clearTimeout(readyDeadline);
    };
    const sendSelection = () => {
      if (!current) return;
      iframe.current?.contentWindow?.postMessage(
        {
          channel: CONNECT_CHANNEL,
          frame_id: frameId,
          type: "select",
          group_id: latest.current.workbench.selected?.groupId || "",
          revision: latest.current.workbench.selected?.revision,
          action: latest.current.workbench.selected?.action,
        },
        current.origin,
      );
    };
    const handle = (event: MessageEvent) => {
      if (
        !current ||
        !acceptsFrameMessage(event, iframe.current?.contentWindow || null, current.origin, frameId)
      )
        return;
      const target = latest.current.workbench.activeInstance;
      if (!target || target.instance_id !== instanceId || target.device_id !== deviceId) return;
      if (event.data.type !== "ready" && !verifiedPeer) return;
      switch (event.data.type) {
        case "ready":
          if (event.data.instance_id !== instanceId || event.data.device_id !== deviceId) return;
          window.clearTimeout(readyDeadline);
          verifiedPeer = true;
          setReady(true);
          sendSelection();
          break;
        case "workspace_dirty":
          if (typeof event.data.dirty === "boolean")
            setWorkspaceDirty(dirtyOwner, event.data.dirty);
          break;
        case "groups": {
          const groups = remoteGroups(event.data.groups);
          if (groups) latest.current.workbench.remember(target, groups);
          break;
        }
        case "selected":
          if (typeof event.data.group_id === "string" && Number.isSafeInteger(event.data.revision))
            latest.current.workbench.reflectSelection(
              instanceId || "",
              event.data.group_id,
              event.data.revision,
            );
          break;
        case "sidebar":
          latest.current.onOpenSidebar();
          break;
        case "locked":
          setWorkspaceDirty(dirtyOwner, false);
          latest.current.workbench.remember(target, null);
          break;
        case "expired":
          fail(t("connect.reopen"));
          break;
      }
    };
    const open = async () => {
      if (!instanceId || !origin) {
        fail(t("connect.unavailable"));
        return;
      }
      const response = await apiJson<OpenFrame>("/api/v1/connect/open", {
        method: "POST",
        body: JSON.stringify({ instance_id: instanceId, frame_id: frameId }),
        signal: AbortSignal.timeout(10000),
      });
      if (cancelled) return;
      if (!response.ok) {
        if (!current) fail(response.error.message);
        else timer = window.setTimeout(() => void open(), 15000);
        return;
      }
      if (
        response.result.origin !== origin ||
        response.result.proof.target_device_id !== deviceId
      ) {
        fail(t("connect.reopen"));
        return;
      }
      window.clearTimeout(deadline);
      deadline = window.setTimeout(
        () => fail(t("connect.reopen")),
        Math.max(0, Date.parse(response.result.proof.expires_at) - Date.now()),
      );
      if (!current) {
        current = response.result;
        setOpened(current);
        setError("");
        readyDeadline = window.setTimeout(() => fail(t("connect.frameUnavailable")), 20000);
      } else {
        current = { ...current, proof: response.result.proof };
        iframe.current?.contentWindow?.postMessage(
          { channel: CONNECT_CHANNEL, frame_id: frameId, type: "renew", proof: current.proof },
          current.origin,
        );
      }
      timer = window.setTimeout(() => void open(), 45000);
    };
    clear(false);
    setError("");
    window.addEventListener("message", handle);
    void open();
    return () => {
      cancelled = true;
      setWorkspaceDirty(dirtyOwner, false);
      window.removeEventListener("message", handle);
      window.clearTimeout(timer);
      window.clearTimeout(deadline);
      window.clearTimeout(readyDeadline);
    };
  }, [instanceId, deviceId, origin, epoch, attempt, t]);

  useEffect(() => {
    if (!opened || !ready) return;
    iframe.current?.contentWindow?.postMessage(
      {
        channel: CONNECT_CHANNEL,
        frame_id: opened.proof.frame_id,
        type: "select",
        group_id: workbench.selected?.groupId || "",
        revision: workbench.selected?.revision,
        action: workbench.selected?.action,
      },
      opened.origin,
    );
  }, [
    opened,
    ready,
    workbench.selected?.groupId,
    workbench.selected?.revision,
    workbench.selected?.action,
  ]);

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="connect-remote-panel">
      <div className="flex min-h-11 shrink-0 items-center gap-3 border-b border-[var(--glass-border-subtle)] px-3 py-1.5">
        <Button variant="ghost" size="sm" className="md:hidden" onClick={onOpenSidebar}>
          {t("workingGroups")}
        </Button>
        <div className="min-w-0 flex-1 truncate text-sm" title={origin || undefined}>
          {instance?.display_name || t("connect.unavailable")}
          {origin ? (
            <span className="ml-2 text-xs text-[var(--color-text-tertiary)]">
              {new URL(origin).host}
            </span>
          ) : null}
        </div>
        {origin ? (
          <a
            className="shrink-0 text-xs underline"
            href={`${origin}/ui/`}
            target="_blank"
            rel="noopener noreferrer"
          >
            {t("connect.openSeparately")}
          </a>
        ) : null}
      </div>
      {opened ? (
        <iframe
          ref={iframe}
          src={opened.url}
          allowFullScreen
          title={instance?.display_name || "CCCC Connect"}
          className={`min-h-0 w-full flex-1 border-0 ${ready ? "" : "invisible"}`}
        />
      ) : null}
      {!ready ? (
        <div
          className={`flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center text-sm ${opened ? "absolute inset-x-0 top-1/2" : ""}`}
        >
          <p role={error ? "alert" : "status"}>{error || t("connecting")}</p>
          {error ? (
            <Button variant="secondary" onClick={() => setAttempt((v) => v + 1)}>
              {t("connect.retry")}
            </Button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
