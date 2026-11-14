import { useState } from "react";
import { DIRECT_PERMANENT_EXPIRY } from "./directConnectionModel";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { DirectInvitationShare } from "./DirectPairingForms";
import { DirectActionDialog } from "./DirectActionDialog";
import {
  directRelationClosed,
  directRelationState,
  type DirectRelation,
} from "./directConnectionModel";

// Same dot language as the Connect sidebar: live, needs attention, or finished.
function stateDot(state: string, closed: boolean) {
  if (state === "online") return "bg-emerald-400";
  if (state === "connectionFailed") return "bg-[var(--color-danger)]";
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
  const closed = directRelationClosed(relation);
  const canApprove = state === "needsApproval";
  const pending = relation.state === "invited" || relation.state === "pending";
  const sharing = state === "invited" && !!invitation;
  const hint = "text-xs text-[var(--color-text-secondary)]";
  return (
    <li className="space-y-2 rounded-lg border border-[var(--glass-border-subtle)] p-3">
      {!sharing && (
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0 flex-1 basis-48">
            <p className="break-words font-medium">
              {relation.remote
                ? `${relation.local.title} ↔ ${relation.remote.name} · ${relation.remote.title}`
                : t("direct.waitingPeer")}
            </p>
            <p role="status" className="mt-1 flex items-center gap-1.5 text-xs">
              <span
                aria-hidden
                className={`h-1.5 w-1.5 shrink-0 rounded-full ${stateDot(state, closed)}`}
              />
              {t(`direct.states.${state}`)}
            </p>
          </div>
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
          </div>
        </div>
      )}
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
        <DirectInvitationShare
          key={invitation}
          text={invitation}
          framed={false}
          onDelete={() => setConfirmRevoke(true)}
        />
      )}
      {state === "invited" && !invitation && <p className={hint}>{t("direct.invitationLost")}</p>}
      {pending && !relation.expired && relation.expires_at && !invitation && (
        <p className={hint}>
          {relation.expires_at === DIRECT_PERMANENT_EXPIRY
            ? t("direct.neverExpires")
            : t("direct.expiresAt", {
                time: new Date(relation.expires_at).toLocaleString(i18n.language),
              })}
        </p>
      )}
      <DirectActionDialog
        open={confirmRevoke}
        busy={busy}
        title={t(
          pending
            ? relation.initiated
              ? "direct.cancelRequest"
              : "direct.cancelInvite"
            : "direct.disconnect",
        )}
        description={t(pending ? "direct.cancelPairHint" : "direct.revokeHint")}
        confirm={t("direct.confirm")}
        onClose={() => setConfirmRevoke(false)}
        onConfirm={() => onAction("revoke", relation.id)}
      />
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
