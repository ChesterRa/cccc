import { useState } from "react";
import { useTranslation } from "react-i18next";
import { DirectActionDialog } from "./DirectActionDialog";
import { DirectInvitationCard } from "./DirectInvitationCard";
import type { DirectSharedInvitation } from "./directConnectionModel";

export function DirectSharedInvitations({
  invitations,
  values,
  busy,
  onCopy,
  onDelete,
  deleteAllowed,
}: {
  invitations: DirectSharedInvitation[];
  values: Record<string, string>;
  busy: boolean;
  onCopy: (id: string) => Promise<string | null>;
  onDelete: (id: string) => Promise<boolean>;
  deleteAllowed: boolean;
}) {
  const { t } = useTranslation("layout");
  const [deleting, setDeleting] = useState<string | null>(null);
  if (!invitations.length) return null;
  return (
    <section className="space-y-3" aria-label={t("direct.sharedInvitations")}>
      <h3 className="font-semibold">
        {t("direct.sharedInvitations")}{" "}
        <span className="text-[var(--color-text-secondary)]">{invitations.length}</span>
      </h3>
      <ul className="grid min-w-0 grid-cols-[repeat(auto-fit,minmax(min(100%,24rem),1fr))] gap-3">
        {invitations.map((invitation) => (
          <li key={invitation.id}>
            <DirectInvitationCard
              invitation={{ ...invitation, reusable: true }}
              cachedText={values[invitation.id]}
              busy={busy}
              onRead={() => onCopy(invitation.id)}
              onDelete={() => setDeleting(invitation.id)}
              deleteDisabled={!deleteAllowed}
            />
          </li>
        ))}
      </ul>
      <DirectActionDialog
        key={deleting || "closed"}
        open={!!deleting}
        busy={busy}
        title={t("direct.deleteInvitation")}
        description={t("direct.deleteInvitationHint")}
        confirm={t("direct.deleteInvitation")}
        onClose={() => setDeleting(null)}
        onConfirm={() => (deleting ? onDelete(deleting) : Promise.resolve(false))}
      />
    </section>
  );
}
