import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiJson } from "../../services/api/base";
import { Button } from "../../components/ui/button";
import { DirectConnectionRow } from "./DirectConnectionRow";
import { DirectInvitationShare, DirectJoinForm, DirectReceiveForm } from "./DirectPairingForms";
import {
  directRelationClosed,
  readDirectInvitation,
  type DirectListener,
  type DirectStatus,
} from "./directConnectionModel";

import { prepareDirectInvitation } from "./prepareDirectInvitation";

export function DirectConnectionsPanel({
  groupId,
  groupTitle = groupId,
}: {
  groupId: string;
  groupTitle?: string;
}) {
  const { t } = useTranslation("layout");
  const [status, setStatus] = useState<DirectStatus | null>(null);
  const [error, setError] = useState("");
  const [submittedId, setSubmittedId] = useState<string | null>(null);
  const [pollFailed, setPollFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
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
        panel.current?.querySelector<HTMLElement>("textarea[readonly]") ||
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
    let disposed = false;
    let controller: AbortController | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      const request = new AbortController();
      controller = request;
      const deadline = setTimeout(() => request.abort(), 10000);
      const result = await apiJson<DirectStatus>(
        `/api/v1/connect/direct?group_id=${encodeURIComponent(groupId)}`,
        { signal: request.signal },
      );
      clearTimeout(deadline);
      if (disposed) return;
      setPollFailed(!result.ok);
      if (result.ok) {
        setStatus(result.result);
        setSubmittedId((id) => (result.result.relations.some((r) => r.id === id) ? null : id));
        setInvitations((previous) =>
          Object.fromEntries(
            Object.entries(previous).filter(([id]) => {
              const relation = result.result.relations.find((r) => r.id === id);
              const seen = observedInvitations.current.has(id);
              if (relation) observedInvitations.current.add(id);
              return relation
                ? relation.state === "invited" && relation.current && !relation.expired
                : !seen;
            }),
          ),
        );
      } else
        setStatus((previous) =>
          previous
            ? {
                ...previous,
                runtime: null,
                relations: previous.relations.map((r) => ({ ...r, online: false })),
              }
            : null,
        );
      timer = setTimeout(() => void poll(), 3000);
    };
    void poll();
    return () => {
      disposed = true;
      controller?.abort();
      clearTimeout(timer);
    };
  }, [groupId, revision]);

  const act = async (action: string, extra: Record<string, unknown> = {}): Promise<boolean> => {
    if (actionRequest.current) return false;
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
      setTask(null);
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
    if ((action === "remove" || action === "revoke") && typeof extra.id === "string") {
      setInvitations((previous) => {
        const next = { ...previous };
        delete next[String(extra.id)];
        return next;
      });
    }
    return true;
  };
  const ready = !!status?.listener && !!status.runtime?.listener && !pollFailed;
  const start = (next: "invite" | "join" | "settings") => {
    setTask(next);
    setError("");
    setSubmittedId(null);
  };
  const closeTask = () => {
    setTask(null);
    setError("");
  };
  const relations = status?.relations || [];
  const current = relations.filter((r) => !directRelationClosed(r));
  const history = relations.filter((r) => !current.includes(r));
  const rows = (items: typeof relations) => (
    <ul className="divide-y divide-[var(--glass-border-subtle)]">
      {items.map((r) => (
        <DirectConnectionRow
          key={r.id}
          relation={r}
          invitation={invitations[r.id]}
          busy={busy}
          onAction={(action, id) => act(action, { id })}
        />
      ))}
    </ul>
  );

  return (
    <div ref={panel} className="space-y-4 text-sm">
      <p className="text-[var(--color-text-secondary)]">{t("direct.description")}</p>
      {(error || pollFailed) && (
        <p role="alert" className="break-words text-[var(--color-danger)]">
          {error || t("direct.statusUnavailable")}
        </p>
      )}
      {submittedId && !relations.some((r) => r.id === submittedId) && (
        <p role="status">{t("direct.requestSubmitted")}</p>
      )}
      {!status && !pollFailed && <p role="status">{t("direct.loading")}</p>}
      {/* Keep a confirmed invitation copyable even if its first status refresh fails. */}
      {Object.entries(invitations)
        .filter(([id]) => !relations.some((r) => r.id === id))
        .map(([id, text]) => (
          <DirectInvitationShare key={id} text={text} />
        ))}
      {current.length > 0 && rows(current)}
      {history.length > 0 && (
        <details>
          <summary className="cursor-pointer text-[var(--color-text-secondary)]">
            {t("direct.history", { count: history.length })}
          </summary>
          <div className="pt-3">{rows(history)}</div>
        </details>
      )}
      {task ? (
        <section className="space-y-3 border-t border-[var(--glass-border-subtle)] pt-4">
          <div className="flex items-start justify-between gap-3">
            <h3 ref={taskHeading} tabIndex={-1} className="font-medium outline-none">
              {t(
                task === "invite"
                  ? "direct.inviteGroup"
                  : task === "join"
                    ? "direct.useInvite"
                    : "direct.configure",
              )}
            </h3>
            <Button size="sm" variant="ghost" disabled={busy} onClick={closeTask}>
              {t("direct.back")}
            </Button>
          </div>
          {task === "join" ? (
            <DirectJoinForm
              groupTitle={groupTitle}
              incoming={incoming}
              setIncoming={setIncoming}
              busy={busy}
              onJoin={() => {
                const preview = readDirectInvitation(incoming);
                if (!preview || Date.parse(preview.expires_at) <= Date.now()) {
                  setError(t("direct.expiredHint"));
                  return;
                }
                void act("join", { invitation: incoming.trim() });
              }}
            />
          ) : (
            <>
              <DirectReceiveForm
                key={task}
                listener={status?.listener || null}
                addresses={status?.addresses || []}
                inviteMode={task === "invite"}
                name={status?.display_name || ""}
                busy={busy}
                onSave={(listener, name, previous) =>
                  void act(task === "invite" ? "invite" : "configure", {
                    listener,
                    display_name: name,
                    expected_listener: previous,
                  })
                }
                onCancel={closeTask}
                onStop={() =>
                  void act("configure", { listener: null, expected_listener: status?.listener })
                }
              />
              {busy && (
                <p role="status">{t(task === "invite" ? "direct.preparing" : "direct.saving")}</p>
              )}
              {status?.runtime?.error && (
                <p className="break-words text-[var(--color-danger)]" role="alert">
                  {t("direct.listenerFailed")} {status.runtime.error}
                </p>
              )}
            </>
          )}
        </section>
      ) : (
        <div className="space-y-2">
          <div className="flex flex-wrap gap-2">
            <Button
              data-direct-start="invite"
              disabled={busy || !status || pollFailed}
              onClick={() => start("invite")}
            >
              {t("direct.inviteGroup")}
            </Button>
            <Button
              variant="outline"
              data-direct-start="join"
              disabled={busy || !status || pollFailed}
              onClick={() => start("join")}
            >
              {t("direct.useInvite")}
            </Button>
          </div>
          <p className="text-xs text-[var(--color-text-secondary)]">{t("direct.chooseHint")}</p>
        </div>
      )}
      {status?.listener && task !== "settings" && task !== "invite" && (
        <div className="border-t border-[var(--glass-border-subtle)] pt-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <p className="break-all text-xs text-[var(--color-text-secondary)]">
              {t(ready ? "direct.listening" : "direct.listenerPending")} · {status.listener.address}
            </p>
            <Button
              variant="ghost"
              size="sm"
              disabled={busy || pollFailed}
              onClick={() => start("settings")}
            >
              {t("direct.configure")}
            </Button>
          </div>
          {status.runtime?.error && (
            <p role="alert" className="break-words">
              {t("direct.listenerFailed")} {status.runtime.error}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
