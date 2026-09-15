import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { DirectInvitationShare } from "./DirectPairingForms";
import {
  directRelationClosed,
  directRelationState,
  type DirectRelation,
} from "./directConnectionModel";

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
  const [confirm, setConfirm] = useState<"approve" | "revoke" | null>(null);
  const state = directRelationState(relation);
  const canRemove = directRelationClosed(relation);
  const canApprove = state === "needsApproval";
  const pending = relation.state === "invited" || relation.state === "pending";
  const confirmation =
    confirm === "revoke" || (confirm === "approve" && canApprove) ? confirm : null;
  return (
    <li className="space-y-2 py-4 first:pt-0">
      <p className="break-words font-medium">
        {relation.remote
          ? `${relation.local.title} ↔ ${relation.remote.name} · ${relation.remote.title}`
          : t("direct.waitingPeer")}
      </p>
      <p role="status" className="text-[var(--color-text-secondary)]">
        {t(`direct.states.${state}`)}
      </p>
      {state === "pending" && (
        <p className="text-xs text-[var(--color-text-secondary)]">{t("direct.pendingHint")}</p>
      )}
      {state === "checkingApproval" && (
        <p className="text-xs text-[var(--color-text-secondary)]">
          {t("direct.checkingApprovalHint")}
        </p>
      )}
      {relation.error &&
        !relation.online &&
        !["revoked", "expired", "replaced"].includes(state) && (
          <div className="space-y-1">
            <p>{t("direct.connectionTrouble")}</p>
            <details className="text-xs text-[var(--color-text-secondary)]">
              <summary className="cursor-pointer">{t("direct.errorDetails")}</summary>
              <p className="break-words py-1">{relation.error}</p>
            </details>
          </div>
        )}
      {state === "invited" && invitation && (
        <DirectInvitationShare key={invitation} text={invitation} />
      )}
      {state === "invited" && !invitation && (
        <p className="text-xs text-[var(--color-text-secondary)]">{t("direct.invitationLost")}</p>
      )}
      {pending && !relation.expired && relation.expires_at && !invitation && (
        <p className="text-xs text-[var(--color-text-secondary)]">
          {t("direct.expiresAt", {
            time: new Date(relation.expires_at).toLocaleString(i18n.language),
          })}
        </p>
      )}
      {relation.remote && (
        <details className="text-xs text-[var(--color-text-secondary)]">
          <summary className="cursor-pointer">{t("direct.identity")}</summary>
          <p className="break-all py-1">
            {relation.local.name} · {relation.local.instance_id}
          </p>
          <p className="break-all py-1">{relation.remote.instance_id}</p>
        </details>
      )}
      {confirmation ? (
        <div className="space-y-2">
          <p>
            {t(
              confirmation === "approve"
                ? "direct.approveHint"
                : pending
                  ? "direct.cancelPairHint"
                  : "direct.revokeHint",
            )}
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              disabled={busy}
              onClick={async () => {
                if (await onAction(confirmation, relation.id)) setConfirm(null);
              }}
            >
              {t("direct.confirm")}
            </Button>
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirm(null)}>
              {t("direct.cancel")}
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex flex-wrap gap-2">
          {canApprove && (
            <Button size="sm" disabled={busy} onClick={() => setConfirm("approve")}>
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
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirm("revoke")}>
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
    </li>
  );
}
