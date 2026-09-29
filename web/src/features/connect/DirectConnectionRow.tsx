import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { DirectInvitationShare } from "./DirectPairingForms";
import {
  directRelationClosed,
  directRelationState,
  type DirectRelation,
} from "./directConnectionModel";

// Same dot language as the Connect sidebar: live, needs attention, or finished.
function stateDot(state: string, closed: boolean) {
  if (state === "online") return "bg-emerald-400";
  if (closed) return "bg-[var(--color-text-tertiary)]";
  return "bg-[var(--color-accent-warning)]";
}

export function DirectConnectionRow({
  relation,
  invitation,
  busy,
  onAction,
}: {
  relation: DirectRelation;
  invitation?: string;
  busy: boolean;
  onAction: (action: string, id: string) => Promise<boolean>;
}) {
  const { t, i18n } = useTranslation("layout");
  const [confirmRevoke, setConfirmRevoke] = useState(false);
  const state = directRelationState(relation);
  const canRemove = directRelationClosed(relation);
  const canApprove = state === "needsApproval";
  const pending = relation.state === "invited" || relation.state === "pending";
  const hint = "text-xs text-[var(--color-text-secondary)]";
  return (
    <li className="space-y-2 rounded-lg border border-[var(--glass-border-subtle)] p-3">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <p className="break-words font-medium">
            {relation.remote
              ? `${relation.local.title} ↔ ${relation.remote.name} · ${relation.remote.title}`
              : t("direct.waitingPeer")}
          </p>
          <p role="status" className="mt-1 flex items-center gap-1.5 text-xs">
            <span
              aria-hidden
              className={`h-1.5 w-1.5 shrink-0 rounded-full ${stateDot(state, canRemove)}`}
            />
            {t(`direct.states.${state}`)}
          </p>
        </div>
        {!confirmRevoke && (
          <div className="flex shrink-0 flex-wrap gap-2">
            {canApprove && (
              <Button
                size="sm"
                disabled={busy}
                onClick={() => void onAction("approve", relation.id)}
              >
                {t("direct.approve")}
              </Button>
            )}
            {canRemove ? (
              <Button
                size="sm"
                variant="ghost"
                disabled={busy}
                onClick={() => void onAction("remove", relation.id)}
              >
                {t("direct.remove")}
              </Button>
            ) : (
              <Button
                size="sm"
                variant="ghost"
                disabled={busy}
                onClick={() => setConfirmRevoke(true)}
              >
                {t(
                  pending
                    ? relation.initiated
                      ? "direct.cancelRequest"
                      : "direct.cancelInvite"
                    : "direct.disconnect",
                )}
              </Button>
            )}
          </div>
        )}
      </div>
      {canApprove && <p className={hint}>{t("direct.approveHint")}</p>}
      {state === "pending" && <p className={hint}>{t("direct.pendingHint")}</p>}
      {state === "checkingApproval" && <p className={hint}>{t("direct.checkingApprovalHint")}</p>}
      {relation.error &&
        !relation.online &&
        !["revoked", "expired", "replaced"].includes(state) && (
          <div className="space-y-1">
            <p>{t("direct.connectionTrouble")}</p>
            <details className={hint}>
              <summary className="cursor-pointer">{t("direct.errorDetails")}</summary>
              <p className="break-words py-1">{relation.error}</p>
            </details>
          </div>
        )}
      {state === "invited" && invitation && (
        <DirectInvitationShare key={invitation} text={invitation} />
      )}
      {state === "invited" && !invitation && <p className={hint}>{t("direct.invitationLost")}</p>}
      {pending && !relation.expired && relation.expires_at && !invitation && (
        <p className={hint}>
          {t("direct.expiresAt", {
            time: new Date(relation.expires_at).toLocaleString(i18n.language),
          })}
        </p>
      )}
      {confirmRevoke && (
        <div className="space-y-2 rounded-md bg-[var(--glass-panel-bg)] p-2">
          <p>{t(pending ? "direct.cancelPairHint" : "direct.revokeHint")}</p>
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              disabled={busy}
              onClick={async () => {
                if (await onAction("revoke", relation.id)) setConfirmRevoke(false);
              }}
            >
              {t("direct.confirm")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={busy}
              onClick={() => setConfirmRevoke(false)}
            >
              {t("direct.cancel")}
            </Button>
          </div>
        </div>
      )}
      {relation.remote && (
        <details className={hint}>
          <summary className="cursor-pointer">{t("direct.identity")}</summary>
          <p className="break-all py-1">
            {relation.local.name} · {relation.local.instance_id}
          </p>
          <p className="break-all py-1">{relation.remote.instance_id}</p>
        </details>
      )}
    </li>
  );
}
