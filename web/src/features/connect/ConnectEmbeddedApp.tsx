import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import App from "../../App";
import { AuthGate } from "../../components/AuthGate";
import { apiJson } from "../../services/api/base";
import { fetchGroups } from "../../services/api";
import { useGroupStore } from "../../stores";
import { acceptsFrameMessage, CONNECT_CHANNEL, readFrameProof, type FrameProof } from "./protocol";

export function ConnectEmbeddedApp() {
  const { t } = useTranslation("layout");
  const [proof, setProof] = useState(() => readFrameProof(window.location));
  const [expired, setExpired] = useState(false);
  const [selection, setSelection] = useState<{ groupId: string; revision: number } | null>(null);
  const proofRef = useRef(proof);
  const send = useCallback((message: Record<string, unknown>) => {
    const current = proofRef.current;
    if (!current || window.parent === window) return;
    window.parent.postMessage(
      { ...message, channel: CONNECT_CHANNEL, frame_id: current.frame_id },
      current.parent_origin,
    );
  }, []);

  useEffect(() => {
    if (!proof || window.parent === window) return;
    let cancelled = false;
    let renewing = false;
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
          previous && previous.revision >= revision ? previous : { groupId: group_id, revision },
        );
      }
      if (
        event.data.type === "renew" &&
        !renewing &&
        event.data.proof?.frame_id === proof.frame_id
      ) {
        renewing = true;
        void apiJson("/api/v1/connect/frame", {
          method: "POST",
          body: JSON.stringify(event.data.proof),
          signal: AbortSignal.timeout(10000),
        })
          .then((result) => {
            if (cancelled || !result.ok) return;
            proofRef.current = event.data.proof as FrameProof;
            setProof(proofRef.current);
          })
          .finally(() => {
            renewing = false;
          });
      }
    };
    window.addEventListener("message", handle);
    send({
      type: "ready",
      instance_id: proof.target_instance_id,
      device_id: proof.target_device_id,
    });
    const deadline = window.setTimeout(
      () => {
        setExpired(true);
        send({ type: "expired" });
      },
      Math.max(0, Date.parse(proof.expires_at) - Date.now()),
    );
    return () => {
      cancelled = true;
      window.removeEventListener("message", handle);
      window.clearTimeout(deadline);
    };
  }, [proof, send]);

  if (!proof || expired || window.parent === window) {
    return (
      <div className="flex h-dvh items-center justify-center p-6 text-center text-sm">
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
        expire={() => setExpired(true)}
      />
    </AuthGate>
  );
}

function AdmittedWorkbench({
  frameId,
  selection,
  send,
  expire,
}: {
  frameId: string;
  selection: { groupId: string; revision: number } | null;
  send: (message: Record<string, unknown>) => void;
  expire: () => void;
}) {
  const selectedGroupId = useGroupStore((state) => state.selectedGroupId);
  const groups = useGroupStore((state) => state.groups);
  const [admitted, setAdmitted] = useState(false);
  const appliedRequest = useRef<number | null>(null);
  const expireRef = useRef(expire);
  expireRef.current = expire;
  useEffect(() => {
    if (!selection || appliedRequest.current === selection.revision) return;
    // Wait for the native list before applying a parent navigation. An absent
    // Group (e.g. deleted since listing) leaves native selection in control.
    if (selection.groupId && !groups.length) return;
    appliedRequest.current = selection.revision;
    if (
      groups.some((g) => g.group_id === selection.groupId) &&
      useGroupStore.getState().selectedGroupId !== selection.groupId
    ) {
      useGroupStore.getState().setSelectedGroupId(selection.groupId);
    }
  }, [selection, groups]);
  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    const refresh = async () => {
      const frame = await apiJson(`/api/v1/connect/frame?frame_id=${encodeURIComponent(frameId)}`, {
        signal: AbortSignal.timeout(10000),
      });
      if (cancelled) return;
      if (!frame.ok) {
        send({ type: "expired" });
        expireRef.current();
        return;
      }
      setAdmitted(true);
      const response = await fetchGroups();
      if (cancelled) return;
      if (response.ok)
        send({
          type: "groups",
          groups: response.result.groups.map((g) => ({
            group_id: g.group_id,
            title: g.title || g.group_id,
            running: Boolean(g.running),
          })),
        });
      timer = window.setTimeout(() => void refresh(), 15000);
    };
    void refresh();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      send({ type: "locked" });
    };
  }, [frameId, send]);
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
    <App connectEmbedded onOpenParentSidebar={() => send({ type: "sidebar" })} />
  ) : null;
}
