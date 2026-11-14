import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiJson } from "../../services/api/base";
import {
  readDirectInvitation,
  type DirectListener,
  type DirectStatus,
} from "./directConnectionModel";
import { useDirectStatus } from "./useDirectStatus";
import { prepareDirectInvitation } from "./prepareDirectInvitation";

export function useDirectActions(groupId: string) {
  const { t } = useTranslation("layout");
  const [error, setError] = useState("");
  const [submittedId, setSubmittedId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  const { status, setStatus, pollFailed } = useDirectStatus(groupId, revision);
  const [task, setTask] = useState<"invite" | "join" | "settings" | null>(null);
  const [incoming, setIncoming] = useState("");
  // Invitations stay only in this open panel, never in browser storage or polling.
  const [invitations, setInvitations] = useState<Record<string, string>>({});
  const observedInvitations = useRef(new Set<string>());
  const actionRequest = useRef<AbortController | null>(null);
  const panel = useRef<HTMLDivElement>(null);
  const taskHeading = useRef<HTMLHeadingElement>(null);
  const previousTask = useRef(task);
  useEffect(() => {
    if (task) taskHeading.current?.focus();
    else if (previousTask.current) {
      const target =
        panel.current?.querySelector<HTMLElement>("textarea[readonly]:not([hidden])") ||
        panel.current?.querySelector<HTMLElement>("button[data-direct-copy]:not(:disabled)") ||
        panel.current?.querySelector<HTMLElement>(
          `button[data-direct-start="${previousTask.current === "join" ? "join" : "invite"}"]`,
        );
      target?.focus();
    }
    previousTask.current = task;
  }, [task]);
  useEffect(
    () => () => {
      actionRequest.current?.abort();
      actionRequest.current = null;
    },
    [],
  );
  useEffect(() => {
    if (!status) return;
    setSubmittedId((id) => (status.relations.some((r) => r.id === id) ? null : id));
    setInvitations((previous) =>
      Object.fromEntries(
        Object.entries(previous).filter(([id]) => {
          const relation = status.relations.find((r) => r.id === id);
          const shared = status.shared_invitations?.find((link) => link.id === id);
          const seen = observedInvitations.current.has(id);
          if (shared) {
            observedInvitations.current.add(id);
            return shared.current && !shared.expired;
          }
          if (relation) observedInvitations.current.add(id);
          return relation
            ? relation.state === "invited" && relation.current && !relation.expired
            : !seen;
        }),
      ),
    );
  }, [status]);

  const act = async (
    action: string,
    extra: Record<string, unknown> = {},
    received?: (text: string) => void,
  ): Promise<boolean> => {
    if (actionRequest.current) return false;
    if (action === "delete_invite" && !status?.capabilities?.invitation_revocation) {
      setError(t("direct.keyRevocationUpgrade"));
      return false;
    }
    setBusy(true);
    setError("");
    setSubmittedId(null);
    const controller = new AbortController();
    actionRequest.current = controller;
    const deadline = setTimeout(() => controller.abort(), 15000);
    const result =
      action === "invite"
        ? await prepareDirectInvitation(
            groupId,
            extra.expected_listener as DirectListener | null,
            extra.listener as DirectListener,
            String(extra.display_name || ""),
            controller.signal,
            typeof extra.expires_in_seconds === "number" ? extra.expires_in_seconds : undefined,
          )
        : await apiJson<{ invitation?: string; id?: string }>("/api/v1/connect/direct", {
            method: "POST",
            signal: controller.signal,
            body: JSON.stringify({ action, group_id: groupId, ...extra }),
          });
    clearTimeout(deadline);
    if (actionRequest.current !== controller) return false;
    actionRequest.current = null;
    setBusy(false);
    // Always read back the outcome, including an uncertain POST response.
    setRevision((n) => n + 1);
    if (!result.ok) {
      const messages: Record<string, string> = {
        direct_listener_failed: "direct.listenerFailed",
        direct_listener_timeout: "direct.prepareTimedOut",
        direct_settings_changed: "direct.settingsChanged",
        direct_upgrade_required: "direct.reusableUpgrade",
        direct_permanent_upgrade_required: "direct.permanentUpgrade",
      };
      setError(
        controller.signal.aborted
          ? t("direct.actionUnconfirmed")
          : messages[result.error.code]
            ? t(messages[result.error.code])
            : result.error.message,
      );
      return false;
    }
    if (result.result.invitation) {
      const text = result.result.invitation;
      const preview = readDirectInvitation(text);
      if (preview) setInvitations((previous) => ({ ...previous, [preview.id]: text }));
      received?.(text);
      if (action !== "copy_invite") setTask(null);
    }
    if (action === "join") {
      setIncoming("");
      setTask(null);
      setSubmittedId(
        "id" in result.result && typeof result.result.id === "string" ? result.result.id : null,
      );
    }
    if (action === "configure") {
      // Configuration acknowledgement is not listener readiness.
      setStatus((previous) =>
        previous
          ? {
              ...previous,
              listener: (extra.listener as DirectStatus["listener"]) ?? null,
              runtime: null,
              display_name:
                typeof extra.display_name === "string" ? extra.display_name : previous.display_name,
            }
          : previous,
      );
      if (task === "settings") setTask(null);
    }
    if ((action === "revoke" || action === "delete_invite") && typeof extra.id === "string") {
      setInvitations((previous) => {
        const next = { ...previous };
        delete next[String(extra.id)];
        return next;
      });
    }
    return true;
  };
  const start = (next: "invite" | "join" | "settings") => {
    setTask(next);
    setError("");
    setSubmittedId(null);
  };
  const closeTask = () => {
    setTask(null);
    setError("");
  };
  const copyInvitation = async (id: string): Promise<string | null> => {
    if (invitations[id]) return invitations[id];
    let text: string | null = null;
    await act("copy_invite", { id }, (value) => {
      text = value;
    });
    return text;
  };
  return {
    error,
    submittedId,
    busy,
    status,
    pollFailed,
    task,
    start,
    incoming,
    setIncoming,
    invitations,
    setError,
    closeTask,
    act,
    copyInvitation,
    panel,
    taskHeading,
  };
}
