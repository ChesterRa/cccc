import { requestWorkspaceNavigation, setWorkspaceDirty } from "../../stores/workspaceNavigation";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiJson } from "../../services/api/base";
import { Button } from "../../components/ui/button";
import {
  acceptsFrameMessage,
  CONNECT_CHANNEL,
  remoteGroups,
  type FrameProof,
  type OpenFrame,
} from "./protocol";
import type { ConnectWorkbench } from "./useConnectWorkbench";

export function ConnectRemotePanel({
  workbench,
  onOpenSidebar,
}: {
  workbench: ConnectWorkbench;
  onOpenSidebar: () => void;
}) {
  const { t } = useTranslation("layout");
  const instance = workbench.retainedInstance;
  const [attempt, setAttempt] = useState(0);
  const [opened, setOpened] = useState<OpenFrame | null>(null);
  const [error, setError] = useState("");
  const [ready, setReady] = useState(false);
  const [paused, setPaused] = useState(false);
  const [lastAttempt, setLastAttempt] = useState(0);
  const iframe = useRef<HTMLIFrameElement>(null);
  const statusButton = useRef<HTMLButtonElement>(null);
  const controls = useRef<{ suspend: () => void; retry: () => void } | null>(null);
  const latest = useRef({ workbench, onOpenSidebar, t });
  latest.current = { workbench, onOpenSidebar, t };
  const instanceId = instance?.instance_id;
  const deviceId = instance?.device_id;
  const publicKey = instance?.public_key;
  const origin = instance?.public_origin;
  const epoch = workbench.selected?.epoch;
  useEffect(() => {
    let cancelled = false;
    let opening = false;
    let timer: number | undefined;
    let deadline: number | undefined;
    let readyDeadline: number | undefined;
    let current: OpenFrame | null = null;
    let pending: FrameProof | null = null;
    let suspended = false;
    let targetAccount = "";
    const dirtyOwner = Symbol("embedded workspace");
    let verifiedPeer = false;
    const frameId = crypto.randomUUID();
    const clear = (forget = true) => {
      setWorkspaceDirty(dirtyOwner, false);
      setOpened(null);
      setReady(false);
      setPaused(false);
      const target = latest.current.workbench.retainedInstance;
      if (forget && target) latest.current.workbench.remember(target, null);
    };
    const fail = (message: string) => {
      clear();
      setError(message);
      current = null;
      pending = null;
      window.clearTimeout(timer);
      window.clearTimeout(deadline);
      window.clearTimeout(readyDeadline);
    };
    const canRecover = () => Boolean(current?.account_id && targetAccount === current.account_id);
    const currentEntryIsAdmitted = () => {
      const entry = latest.current.workbench;
      return Boolean(
        entry.available &&
        entry.activeAccountId === current?.account_id &&
        entry.activeDirectoryExpiresAt &&
        Date.parse(entry.activeDirectoryExpiresAt) > Date.now(),
      );
    };
    const post = (message: Record<string, unknown>) => {
      if (!current) return;
      iframe.current?.contentWindow?.postMessage(
        { ...message, channel: CONNECT_CHANNEL, frame_id: frameId },
        current.origin,
      );
    };
    const suspend = () => {
      if (!current) return;
      if (suspended) {
        // A newer loss of entry authority invalidates an in-flight recovery,
        // even though the page is already visually suspended.
        pending = null;
        post({ type: "suspend" });
        return;
      }
      if (!canRecover()) {
        fail(latest.current.t("connect.reopen"));
        return;
      }
      suspended = true;
      pending = null;
      setPaused(true);
      post({ type: "suspend" });
      iframe.current?.blur();
      window.requestAnimationFrame(() => statusButton.current?.focus());
    };
    const armDeadline = (proof: FrameProof) => {
      window.clearTimeout(deadline);
      deadline = window.setTimeout(suspend, Math.max(0, Date.parse(proof.expires_at) - Date.now()));
    };
    const sendSelection = () => {
      if (!current || suspended) return;
      post({
        type: "select",
        group_id: latest.current.workbench.selected?.groupId || "",
        revision: latest.current.workbench.selected?.revision,
        action: latest.current.workbench.selected?.action,
      });
    };
    const handle = (event: MessageEvent) => {
      if (
        !current ||
        !acceptsFrameMessage(event, iframe.current?.contentWindow || null, current.origin, frameId)
      )
        return;
      const target = latest.current.workbench.retainedInstance;
      if (
        !target ||
        target.instance_id !== instanceId ||
        target.device_id !== deviceId ||
        target.public_key !== publicKey
      )
        return;
      if (event.data.type !== "ready" && !verifiedPeer) return;
      switch (event.data.type) {
        case "ready":
          if (event.data.instance_id !== instanceId || event.data.device_id !== deviceId) return;
          window.clearTimeout(readyDeadline);
          verifiedPeer = true;
          setReady(true);
          sendSelection();
          break;
        case "admitted":
          if (typeof event.data.account_id !== "string" || !event.data.account_id) return;
          targetAccount = event.data.account_id;
          if (current.account_id && current.account_id !== targetAccount)
            fail(latest.current.t("connect.reopen"));
          break;
        case "workspace_dirty":
          if (typeof event.data.dirty === "boolean")
            setWorkspaceDirty(dirtyOwner, event.data.dirty);
          break;
        case "groups": {
          const groups = remoteGroups(event.data.groups);
          if (groups && !suspended) latest.current.workbench.remember(target, groups);
          break;
        }
        case "selected":
          if (
            !suspended &&
            typeof event.data.group_id === "string" &&
            Number.isSafeInteger(event.data.revision)
          )
            latest.current.workbench.reflectSelection(
              instanceId || "",
              event.data.group_id,
              event.data.revision,
            );
          break;
        case "sidebar":
          if (!suspended) latest.current.onOpenSidebar();
          break;
        case "locked":
          // A login gate can unmount the workbench while the page still owns
          // failed sends and drafts in its page-level stores.
          latest.current.workbench.remember(target, null);
          break;
        case "expired":
          if (event.data.recoverable === true) suspend();
          else fail(latest.current.t("connect.reopen"));
          break;
        case "identity_changed":
          fail(latest.current.t("connect.reopen"));
          break;
        case "renewed":
          if (!suspended && pending && event.data.nonce === pending.nonce) {
            current = { ...current, proof: pending };
            pending = null;
            armDeadline(current.proof);
          }
          break;
        case "resume_ready":
          if (
            suspended &&
            pending &&
            event.data.nonce === pending.nonce &&
            currentEntryIsAdmitted()
          )
            post({ type: "resume_commit", nonce: pending.nonce });
          break;
        case "resumed":
          if (
            suspended &&
            pending &&
            event.data.nonce === pending.nonce &&
            currentEntryIsAdmitted()
          ) {
            current = { ...current, proof: pending };
            pending = null;
            suspended = false;
            setPaused(false);
            armDeadline(current.proof);
            sendSelection();
          } else if (suspended) post({ type: "suspend" });
          break;
        case "reload":
          requestWorkspaceNavigation(() => setAttempt((value) => value + 1));
          break;
      }
    };
    const schedule = (ms: number) => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => void open(), ms);
    };
    const open = async () => {
      if (opening || cancelled) return;
      if (!instanceId || !origin) {
        fail(latest.current.t("connect.unavailable"));
        return;
      }
      if (!latest.current.workbench.available) {
        if (current) suspend();
        schedule(15000);
        return;
      }
      opening = true;
      setLastAttempt(Date.now());
      const response = await apiJson<OpenFrame>("/api/v1/connect/open", {
        method: "POST",
        body: JSON.stringify({ instance_id: instanceId, frame_id: frameId }),
        signal: AbortSignal.timeout(10000),
      });
      opening = false;
      if (cancelled) return;
      if (!response.ok) {
        if (!current) fail(response.error.message);
        else schedule(15000);
        return;
      }
      const next = response.result;
      if (
        next.origin !== origin ||
        next.proof.target_device_id !== deviceId ||
        (current?.account_id && next.account_id !== current.account_id) ||
        (next.account_id &&
          latest.current.workbench.activeAccountId &&
          next.account_id !== latest.current.workbench.activeAccountId)
      ) {
        fail(latest.current.t("connect.reopen"));
        return;
      }
      if (!current) {
        current = next;
        setOpened(current);
        setError("");
        armDeadline(current.proof);
        readyDeadline = window.setTimeout(
          () => fail(latest.current.t("connect.frameUnavailable")),
          20000,
        );
      } else {
        pending = next.proof;
        if (suspended) {
          post({ type: "resume", proof: pending, account_id: current.account_id });
        } else {
          post({ type: "renew", proof: pending });
          // Older targets cannot acknowledge renewal or restore a retained
          // document. Preserve their existing close-and-reopen behavior.
          if (!canRecover()) {
            current = { ...current, proof: pending };
            pending = null;
            armDeadline(current.proof);
          }
        }
      }
      schedule(suspended ? 15000 : 45000);
    };
    controls.current = { suspend, retry: () => void open() };
    clear(false);
    setError("");
    window.addEventListener("message", handle);
    void open();
    return () => {
      cancelled = true;
      if (controls.current?.suspend === suspend) controls.current = null;
      setWorkspaceDirty(dirtyOwner, false);
      window.removeEventListener("message", handle);
      window.clearTimeout(timer);
      window.clearTimeout(deadline);
      window.clearTimeout(readyDeadline);
    };
  }, [instanceId, deviceId, publicKey, origin, epoch, attempt]);

  useLayoutEffect(() => {
    if (!workbench.available) controls.current?.suspend();
    else if (paused) controls.current?.retry();
  }, [workbench.available, paused]);

  useEffect(() => {
    if (!opened || !ready || paused) return;
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
    paused,
    workbench.selected?.groupId,
    workbench.selected?.revision,
    workbench.selected?.action,
  ]);

  return (
    <div className="relative flex h-full min-h-0 flex-col" data-testid="connect-remote-panel">
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
          inert={paused || !workbench.available}
          className={`min-h-0 w-full flex-1 border-0 ${ready && !paused && workbench.available ? "" : "invisible"}`}
        />
      ) : null}
      {paused ? (
        <div className="absolute inset-x-0 bottom-0 top-11 flex flex-col items-center justify-center gap-3 bg-[var(--color-bg-primary)] p-6 text-center text-sm">
          <p role="status">{t("connect.reconnecting")}</p>
          {lastAttempt ? (
            <p className="text-xs text-[var(--color-text-tertiary)]">
              {t("connect.lastAttempt", { time: new Date(lastAttempt).toLocaleTimeString() })}
            </p>
          ) : null}
          <Button
            ref={statusButton}
            variant="secondary"
            onClick={() => requestWorkspaceNavigation(() => workbench.selectLocal())}
          >
            {t("connect.closePage")}
          </Button>
        </div>
      ) : !ready ? (
        <div
          className={`flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center text-sm ${opened ? "absolute inset-x-0 top-1/2" : ""}`}
        >
          <p role={error ? "alert" : "status"} data-status-tone={error ? "error" : "muted"}>
            {error || t("connecting")}
          </p>
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
