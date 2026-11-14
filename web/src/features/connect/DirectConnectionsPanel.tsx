import { useDirectActions } from "./useDirectActions";
import { DirectSharedInvitations } from "./DirectSharedInvitations";
import { DirectConnectionTask } from "./DirectConnectionTask";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { DirectConnectionStart } from "./DirectConnectionStart";
import { DirectConnectionsList } from "./DirectConnectionsList";
import { DirectInvitationShare } from "./DirectPairingForms";
import { directRelationClosed } from "./directConnectionModel";

export function DirectConnectionsPanel({
  groupId,
  groupTitle = groupId,
  fill = false,
}: {
  groupId: string;
  groupTitle?: string;
  fill?: boolean;
}) {
  const { t } = useTranslation("layout");
  const actions = useDirectActions(groupId);
  const {
    error,
    submittedId,
    busy,
    status,
    pollFailed,
    task,
    start,
    invitations,
    act,
    copyInvitation,
    panel,
  } = actions;
  const ready = !!status?.listener && !!status.runtime?.listener && !pollFailed;
  const relations = status?.relations || [];
  const current = relations.filter((r) => !directRelationClosed(r));

  return (
    <div
      ref={panel}
      className={fill ? "flex min-h-full flex-col gap-4 text-sm" : "space-y-4 text-sm"}
    >
      {(error || pollFailed) && (
        <p role="alert" className="break-words text-[var(--color-danger)]">
          {error || t("direct.statusUnavailable")}
        </p>
      )}
      {submittedId && !relations.some((r) => r.id === submittedId) && (
        <p role="status">{t("direct.requestSubmitted")}</p>
      )}
      {!status && !pollFailed && <p role="status">{t("direct.loading")}</p>}
      {status && !status.capabilities?.reusable_invitations && (
        <div
          role="status"
          className="space-y-1 rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] p-4"
        >
          <p className="font-medium">{t("direct.reusableUnavailable")}</p>
          <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
            {t("direct.reusableUnavailableHint")}
          </p>
        </div>
      )}
      {task ? (
        <DirectConnectionTask groupTitle={groupTitle} actions={actions} />
      ) : (
        <DirectConnectionStart
          empty={
            !!status &&
            current.length === 0 &&
            !status.shared_invitations?.length &&
            !submittedId &&
            Object.keys(invitations).length === 0
          }
          disabled={busy || !status || pollFailed}
          onStart={start}
        />
      )}
      {/* Keep a confirmed invitation copyable even if its first status refresh fails. */}
      {Object.entries(invitations)
        .filter(
          ([id]) =>
            !relations.some((r) => r.id === id) &&
            !status?.shared_invitations?.some((link) => link.id === id),
        )
        .map(([id, text]) => (
          <DirectInvitationShare key={id} text={text} />
        ))}
      <DirectSharedInvitations
        invitations={status?.shared_invitations || []}
        values={invitations}
        busy={busy}
        onCopy={copyInvitation}
        onDelete={(id) => act("delete_invite", { id })}
        deleteAllowed={!!status?.capabilities?.invitation_revocation}
      />
      {/* Live relations stay visible in a task: approvals and uncertain outcomes surface here. */}
      {current.length > 0 && (
        <DirectConnectionsList
          relations={current}
          invitations={invitations}
          busy={busy}
          onAction={(action, id) => act(action, { id })}
        />
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
