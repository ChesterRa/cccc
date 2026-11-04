import {
  requestWorkspaceNavigation,
  useWorkspaceNavigation,
} from "../../stores/workspaceNavigation";
import {
  groupConnectionCount,
  type GroupConnectionSummary,
  type ConnectStatusResponse,
} from "./protocol";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import App from "../../App";
import { AuthGate } from "../../components/AuthGate";
import { apiFrameControl, apiJson } from "../../services/api/base";
import { fetchGroups } from "../../services/api";
import { useGroupStore } from "../../stores";
import { useComposerStore } from "../../stores/useComposerStore";
import { usePendingPageWork } from "../../stores/usePendingPageWork";
import { useModalStore } from "../../stores/useModalStore";
import { hasUnsentComposerWork } from "./composerWork";
import { isTransientApiFailure } from "../../utils/transientApiFailure";
import { acceptsFrameMessage, CONNECT_CHANNEL, readFrameProof, type FrameProof } from "./protocol";
import {
  beginFrameAccess,
  endFrameAccess,
  frameAccessPaused,
  pauseFrameAccess,
  resumeFrameAccess,
  subscribeFrameAccess,
} from "./frameAccess";

const FRAME_CHECK_INTERVAL_MS = 15000;

type EmbeddedSelection = { groupId: string; revision: number; action?: "connections" };

export function ConnectEmbeddedApp() {
  const { t } = useTranslation("layout");
  const [proof] = useState(() => {
    const initial = readFrameProof(window.location);
    if (initial && window.parent !== window) beginFrameAccess(initial.expires_at);
    return initial;
  });
  const [expired, setExpired] = useState(false);
  const [selection, setSelection] = useState<EmbeddedSelection | null>(null);
  const workspaceDirty = useWorkspaceNavigation((state) => state.owners.size > 0);
  const composerWork = useComposerStore(hasUnsentComposerWork);
  const pendingPageWork = usePendingPageWork((state) => state.owners.size > 0);
  const proofRef = useRef(proof);
  const accountIdRef = useRef("");
  const admittedRef = useRef(false);
  const send = useCallback((message: Record<string, unknown>) => {
    const current = proofRef.current;
    if (!current || window.parent === window) return;
    window.parent.postMessage(
      { ...message, channel: CONNECT_CHANNEL, frame_id: current.frame_id },
      current.parent_origin,
    );
  }, []);

  const expire = useCallback(() => {
    if (admittedRef.current && accountIdRef.current) {
      pauseFrameAccess();
    } else {
      setExpired(true);
      send({ type: "expired" });
    }
  }, [send]);

  const terminate = useCallback(() => {
    // Keep the request and media guard closed until the entry actually removes
    // this document. Revocation must never briefly resume its background work.
    send({ type: "identity_changed" });
    pauseFrameAccess();
    setExpired(true);
  }, [send]);

  const rememberAdmission = useCallback((accountId: string) => {
    admittedRef.current = true;
    if (accountId && !accountIdRef.current) accountIdRef.current = accountId;
  }, []);

  useEffect(() => {
    if (!proof || window.parent === window) return;
    // StrictMode can replay this effect after its cleanup without rerunning
    // the state initializer that first enabled the request guard.
    beginFrameAccess(proofRef.current?.expires_at || proof.expires_at);
    let cancelled = false;
    let renewing = false;
    let deadline: number | undefined;
    let pending: FrameProof | null = null;
    let generation = 0;
    const armDeadline = (current: FrameProof) => {
      window.clearTimeout(deadline);
      deadline = window.setTimeout(
        expire,
        Math.max(0, Date.parse(current.expires_at) - Date.now()),
      );
    };
    const sameBinding = (next: FrameProof) => {
      const current = proofRef.current;
      return (
        current &&
        [
          "frame_id",
          "account_origin",
          "source_instance_id",
          "source_device_id",
          "target_instance_id",
          "target_device_id",
          "parent_origin",
        ].every((key) => next[key as keyof FrameProof] === current[key as keyof FrameProof])
      );
    };
    const handle = (event: MessageEvent) => {
      if (!acceptsFrameMessage(event, window.parent, proof.parent_origin, proof.frame_id)) return;
      if (
        event.data.type === "select" &&
        typeof event.data.group_id === "string" &&
        Number.isSafeInteger(event.data.revision) &&
        event.data.revision > 0
      ) {
        const { group_id, revision } = event.data;
        setSelection((previous) =>
          previous && previous.revision >= revision
            ? previous
            : {
                groupId: group_id,
                revision,
                action: event.data.action === "connections" ? "connections" : undefined,
              },
        );
      }
      if (event.data.type === "suspend") {
        generation += 1;
        pending = null;
        expire();
      }
      if (
        event.data.type === "resume_commit" &&
        pending &&
        frameAccessPaused() &&
        event.data.nonce === pending.nonce
      ) {
        const next = pending;
        pending = null;
        if (Date.parse(next.expires_at) <= Date.now()) return;
        proofRef.current = next;
        resumeFrameAccess(next.expires_at);
        armDeadline(next);
        send({ type: "resumed", nonce: next.nonce });
      }
      const next = event.data.proof as FrameProof | undefined;
      if (
        (event.data.type === "renew" || event.data.type === "resume") &&
        !renewing &&
        next &&
        sameBinding(next)
      ) {
        const recovering = event.data.type === "resume";
        if (
          recovering &&
          (!accountIdRef.current || event.data.account_id !== accountIdRef.current)
        ) {
          send({ type: "identity_changed" });
          return;
        }
        if (!recovering && frameAccessPaused()) return;
        renewing = true;
        const requestGeneration = ++generation;
        void (async () => {
          if (recovering) {
            const status = await apiFrameControl<ConnectStatusResponse>("/api/v1/connect", {
              signal: AbortSignal.timeout(10000),
            });
            if (cancelled || requestGeneration !== generation) return;
            if (!status.ok) {
              if (isAccessRejection(status.error)) terminate();
              return;
            }
            const account = status.result.connect?.directory?.account_id;
            if (!account) return;
            if (account !== accountIdRef.current) {
              terminate();
              return;
            }
          }
          const result = await apiFrameControl(
            recovering ? "/api/v1/connect/frame/resume" : "/api/v1/connect/frame",
            {
              method: "POST",
              body: JSON.stringify(
                recovering ? { proof: next, account_id: accountIdRef.current } : next,
              ),
              signal: AbortSignal.timeout(10000),
            },
          );
          if (cancelled || requestGeneration !== generation) return;
          if (!result.ok) {
            if (recovering && isAccessRejection(result.error)) terminate();
            return;
          }
          if (recovering) {
            pending = next;
            send({ type: "resume_ready", nonce: next.nonce });
          } else {
            // A renewal begun before suspension must not reopen the page
            // without the entry's fresh account and access checks.
            if (frameAccessPaused()) return;
            proofRef.current = next;
            resumeFrameAccess(next.expires_at);
            armDeadline(next);
            send({ type: "renewed", nonce: next.nonce });
          }
        })().finally(() => {
          renewing = false;
        });
      }
    };
    window.addEventListener("message", handle);
    const unsubscribe = subscribeFrameAccess(() => {
      if (frameAccessPaused()) send({ type: "expired", recoverable: true });
    });
    const checkDeadline = () => {
      frameAccessPaused();
    };
    window.addEventListener("focus", checkDeadline);
    document.addEventListener("visibilitychange", checkDeadline);
    send({
      type: "ready",
      instance_id: proof.target_instance_id,
      device_id: proof.target_device_id,
    });
    armDeadline(proof);
    return () => {
      cancelled = true;
      unsubscribe();
      endFrameAccess();
      window.removeEventListener("focus", checkDeadline);
      document.removeEventListener("visibilitychange", checkDeadline);
      window.removeEventListener("message", handle);
      window.clearTimeout(deadline);
    };
  }, [proof, send, expire, terminate]);

  // Switching instances destroys this document without a beforeunload event.
  // Keep reporting through the sign-in gate: a send can fail after it unmounts
  // the workbench, and its recovery snapshot is still owned by this page.
  useEffect(() => {
    send({ type: "workspace_dirty", dirty: workspaceDirty || pendingPageWork || composerWork });
  }, [workspaceDirty, pendingPageWork, composerWork, send]);

  if (!proof || expired || window.parent === window) {
    return (
      <div role="status" className="flex h-dvh items-center justify-center p-6 text-center text-sm">
        {t("connect.reopen")}
      </div>
    );
  }
  return (
    <AuthGate requireAdmin>
      <AdmittedWorkbench
        frameId={proof.frame_id}
        selection={selection}
        send={send}
        expire={expire}
        rememberAdmission={rememberAdmission}
      />
    </AuthGate>
  );
}

function AdmittedWorkbench({
  frameId,
  selection,
  send,
  expire,
  rememberAdmission,
}: {
  frameId: string;
  selection: EmbeddedSelection | null;
  send: (message: Record<string, unknown>) => void;
  expire: () => void;
  rememberAdmission: (accountId: string) => void;
}) {
  const { t } = useTranslation("layout");
  const selectedGroupId = useGroupStore((state) => state.selectedGroupId);
  const groups = useGroupStore((state) => state.groups);
  const lastSummary = useRef<GroupConnectionSummary | null>(null);
  const [accountLabel, setAccountLabel] = useState<string | null>(null);
  const [admitted, setAdmitted] = useState(false);
  const appliedRequest = useRef<number | null>(null);
  const expireRef = useRef(expire);
  expireRef.current = expire;
  useEffect(() => {
    if (!admitted || !selection || appliedRequest.current === selection.revision) return;
    // Wait for the native list before applying a parent navigation. An absent
    // Group (e.g. deleted since listing) leaves native selection in control.
    if (selection.groupId && !groups.length) return;
    appliedRequest.current = selection.revision;
    const targetExists = groups.some((g) => g.group_id === selection.groupId);
    const apply = () => {
      // A new navigation closes the previous Group's dialog. Repeated frame
      // messages in the same revision cannot reopen a dialog the user dismissed.
      useModalStore
        .getState()
        .setGroupConnections(
          targetExists && selection.action === "connections" ? selection.groupId : null,
        );
      if (targetExists && useGroupStore.getState().selectedGroupId !== selection.groupId) {
        useGroupStore.getState().setSelectedGroupId(selection.groupId);
      }
    };
    if (targetExists && useGroupStore.getState().selectedGroupId !== selection.groupId)
      requestWorkspaceNavigation(apply);
    else apply();
  }, [selection, groups, admitted]);
  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    const refresh = async () => {
      const readFrame = frameAccessPaused() ? apiFrameControl : apiJson;
      const frame = await readFrame<{ account_id?: string }>(
        `/api/v1/connect/frame?frame_id=${encodeURIComponent(frameId)}`,
        { signal: AbortSignal.timeout(10000) },
      );
      if (cancelled) return;
      if (!frame.ok) {
        if (isTransientApiFailure(frame.error)) {
          // A transient transport failure does not revoke the proof. Its own
          // deadline still suspends this workbench if revalidation never succeeds.
          timer = window.setTimeout(() => void refresh(), FRAME_CHECK_INTERVAL_MS);
          return;
        }
        if (isAccessRejection(frame.error)) {
          // The target's login gate owns a revoked administrator session.
          // Do not destroy page-level recovery state while it asks for login.
          timer = window.setTimeout(() => void refresh(), FRAME_CHECK_INTERVAL_MS);
          return;
        }
        expireRef.current();
        // Recovery keeps this component mounted. Keep checking so later
        // revocation is still detected after a successful resume.
        timer = window.setTimeout(() => void refresh(), FRAME_CHECK_INTERVAL_MS);
        return;
      }
      if (frameAccessPaused()) {
        timer = window.setTimeout(() => void refresh(), FRAME_CHECK_INTERVAL_MS);
        return;
      }
      rememberAdmission(frame.result.account_id || "");
      if (frame.result.account_id) send({ type: "admitted", account_id: frame.result.account_id });
      setAdmitted(true);
      const [response, status] = await Promise.all([
        fetchGroups(),
        apiJson<ConnectStatusResponse>("/api/v1/connect", { signal: AbortSignal.timeout(5000) }),
      ]);
      if (cancelled) return;
      if (status.ok) {
        setAccountLabel(status.result.account_label || null);
        lastSummary.current =
          status.result.group_connections || (status.result.connect ? lastSummary.current : null);
      }
      if (response.ok)
        send({
          type: "groups",
          groups: response.result.groups.map((g) => ({
            group_id: g.group_id,
            title: g.title || g.group_id,
            running: Boolean(g.running),
            connection: groupConnectionCount(lastSummary.current, g.group_id),
          })),
        });
      timer = window.setTimeout(() => void refresh(), FRAME_CHECK_INTERVAL_MS);
    };
    void refresh();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      send({ type: "locked" });
    };
  }, [frameId, send, rememberAdmission]);
  useEffect(() => {
    if (
      !admitted ||
      !selection ||
      appliedRequest.current !== selection.revision ||
      selectedGroupId !== useGroupStore.getState().selectedGroupId
    )
      return;
    send({ type: "selected", group_id: selectedGroupId, revision: selection.revision });
  }, [selectedGroupId, selection, admitted, send]);
  return admitted ? (
    <App
      connectEmbedded
      embeddedAccountLabel={accountLabel}
      onOpenParentSidebar={() => send({ type: "sidebar" })}
    />
  ) : (
    <div role="status" className="flex h-dvh items-center justify-center p-6 text-center text-sm">
      {t("connecting")}
    </div>
  );
}

function isAccessRejection(error: { code: string; httpStatus?: number }) {
  return (
    error.httpStatus === 401 ||
    ["auth_required", "unauthorized", "admin_required", "permission_denied"].includes(error.code)
  );
}
