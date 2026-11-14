import { useTranslation } from "react-i18next";
import { Link2, Trash2 } from "lucide-react";
import { LcdIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { DIRECT_PERMANENT_EXPIRY } from "./directConnectionModel";

export type InvitationMetadata = {
  id: string;
  expires_at: string;
  reusable: boolean;
  uses?: number;
  current?: boolean;
  expired?: boolean;
};

export function DirectInvitationSummary({
  invitation,
  expired,
  busy,
  deleteDisabled,
  onDelete,
}: {
  invitation: InvitationMetadata;
  expired: boolean;
  busy: boolean;
  deleteDisabled: boolean;
  onDelete?: () => void;
}) {
  const { t, i18n } = useTranslation("layout");
  return (
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div className="flex min-w-0 flex-1 items-start gap-3">
        <span className="flex h-10 w-10 shrink-0 items-center justify-center border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)]">
          <LcdIcon icon={Link2} glyph="link" className="h-5 w-5" aria-hidden="true" />
        </span>
        <div className="min-w-0 space-y-1">
          <p className="font-semibold">
            {t(invitation.reusable ? "direct.reusableInvitation" : "direct.legacyInvitation")}
          </p>
          <p className="text-sm text-[var(--color-text-secondary)]">
            {expired
              ? t("direct.expiredHint")
              : invitation.expires_at === DIRECT_PERMANENT_EXPIRY
                ? t("direct.neverExpires")
                : t("direct.expiresAt", {
                    time: new Date(invitation.expires_at).toLocaleString(i18n.language, {
                      dateStyle: "medium",
                      timeStyle: "short",
                    }),
                  })}
          </p>
          <p className="font-mono text-xs text-[var(--color-text-muted)]" title={invitation.id}>
            KEY · {invitation.id.slice(-12)}
          </p>
        </div>
      </div>
      <div className="flex shrink-0 items-start gap-2">
        {typeof invitation.uses === "number" && (
          <div
            className="border border-[var(--glass-border)] bg-[var(--glass-tab-bg)] px-3 py-2 text-center"
            title={t("direct.keyUsageHint")}
            aria-label={t("direct.invitationUses", { count: invitation.uses })}
          >
            <p className="text-xs text-[var(--color-text-secondary)]">{t("direct.keyUsesLabel")}</p>
            <p className="text-xl font-semibold tabular-nums">
              {invitation.uses.toLocaleString(i18n.language)}
            </p>
          </div>
        )}
        {onDelete && (
          <Button
            variant="ghost"
            size="iconTouch"
            aria-label={t(invitation.reusable ? "direct.deleteInvitation" : "direct.cancelInvite")}
            disabled={busy || deleteDisabled}
            title={deleteDisabled ? t("direct.keyRevocationUpgrade") : undefined}
            onClick={onDelete}
          >
            <LcdIcon icon={Trash2} glyph="trash" className="h-4 w-4" aria-hidden="true" />
          </Button>
        )}
      </div>
    </div>
  );
}
