import { useId } from "react";
import { useTranslation } from "react-i18next";
import { ArrowRight, Clock3, Network } from "lucide-react";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/textarea";
import { readDirectInvitation } from "./directConnectionModel";
import "./directInvitationInput.css";

export function DirectJoinForm({
  groupTitle,
  incoming,
  setIncoming,
  busy,
  onJoin,
}: {
  groupTitle: string;
  incoming: string;
  setIncoming: (value: string) => void;
  busy: boolean;
  onJoin: () => void;
}) {
  const { t, i18n } = useTranslation("layout");
  const id = useId();
  const preview = readDirectInvitation(incoming);
  const expired = preview && Date.parse(preview.expires_at) <= Date.now();
  const invalid = !!incoming.trim() && !preview;
  return (
    <form
      className="min-w-0 space-y-4"
      onSubmit={(event) => {
        event.preventDefault();
        if (!busy && preview && !expired) onJoin();
      }}
    >
      <div className="space-y-2">
        <label htmlFor={id} className="block text-sm font-medium">
          {t("direct.pasteInvite")}
        </label>
        <Textarea
          id={id}
          rows={2}
          maxLength={8192}
          value={incoming}
          disabled={busy}
          autoComplete="off"
          spellCheck={false}
          placeholder="cccc-direct:…"
          className="direct-invitation-input min-h-20 resize-y font-mono text-xs leading-5"
          aria-invalid={invalid || !!expired}
          aria-describedby={invalid ? `${id}-error` : expired ? `${id}-expiry` : undefined}
          onChange={(event) => setIncoming(event.target.value)}
        />
        {invalid && (
          <p id={`${id}-error`} role="alert" className="text-xs text-[var(--color-danger)]">
            {t("direct.invalidInvitation")}
          </p>
        )}
      </div>
      {preview && (
        <section
          aria-label={t("direct.reviewPair")}
          className="min-w-0 overflow-hidden rounded-xl border border-[var(--glass-border-subtle)]"
        >
          <div className="grid min-w-0 gap-3 bg-[var(--glass-panel-bg)] p-4 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] sm:items-center">
            <div className="min-w-0 space-y-1">
              <p className="text-xs text-[var(--color-text-secondary)]">
                {t("groupConnections.localGroup")}
              </p>
              <p className="break-words text-sm font-semibold [overflow-wrap:anywhere]">
                {groupTitle}
              </p>
            </div>
            <ArrowRight
              aria-hidden="true"
              className="h-4 w-4 rotate-90 text-[var(--color-text-tertiary)] sm:rotate-0"
            />
            <div className="min-w-0 space-y-1">
              <p className="text-xs text-[var(--color-text-secondary)]">
                {t("direct.invitingGroup")}
              </p>
              <p className="break-words text-sm font-semibold [overflow-wrap:anywhere]">
                {preview.host.title}
              </p>
              <p className="break-words text-xs text-[var(--color-text-secondary)] [overflow-wrap:anywhere]">
                {preview.host.name}
              </p>
            </div>
          </div>
          <div className="space-y-2 border-t border-[var(--glass-border-subtle)] px-4 py-3 text-xs text-[var(--color-text-secondary)]">
            <p className="flex min-w-0 items-start gap-2">
              <Network aria-hidden="true" className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              <span className="min-w-0 break-all font-mono">{preview.address}</span>
            </p>
            <p
              id={`${id}-expiry`}
              role={expired ? "alert" : undefined}
              className={`flex items-start gap-2 ${expired ? "text-[var(--color-danger)]" : ""}`}
            >
              <Clock3 aria-hidden="true" className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              <span>
                {expired
                  ? t("direct.expiredHint")
                  : t("direct.expiresAt", {
                      time: new Date(preview.expires_at).toLocaleString(i18n.language),
                    })}
              </span>
            </p>
            <details>
              <summary className="cursor-pointer">{t("direct.identity")}</summary>
              <p className="break-all pt-2 font-mono">{preview.host.instance_id}</p>
            </details>
          </div>
        </section>
      )}
      <div className="sticky bottom-0 z-10 flex justify-end border-t border-[var(--glass-border-subtle)] bg-[var(--glass-bg)] py-3">
        <Button
          type="submit"
          className="w-full shrink-0 sm:w-auto"
          disabled={busy || !preview || !!expired}
        >
          {t(busy ? "direct.requesting" : "direct.request")}
          <ArrowRight aria-hidden="true" className="h-4 w-4" />
        </Button>
      </div>
    </form>
  );
}
