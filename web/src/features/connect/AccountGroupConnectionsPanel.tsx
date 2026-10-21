import { ExternalLink, Link2, RefreshCw, TriangleAlert } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { useAccountGroupConnections } from "./useAccountGroupConnections";

/** One status area and one primary action, shared by the dialog and settings. */
export function AccountGroupConnectionsPanel({
  groupId,
  invitation = "",
  fill = false,
  onOpenAccount,
}: {
  groupId: string;
  invitation?: string;
  fill?: boolean;
  onOpenAccount: () => void;
}) {
  const { t } = useTranslation("layout");
  const {
    value,
    error,
    busy,
    prepared,
    links,
    state,
    confirming,
    headline,
    accountLink,
    select,
    refresh,
  } = useAccountGroupConnections(groupId, invitation);
  const unlinked = state === "not_linked";
  const failed = state === "unavailable" || !!error;
  const empty = state === "ready" && links.length === 0;
  const canSelect = !busy && state === "ready" && !!value?.account_id && !!groupId;
  const primary = unlinked ? (
    <Button onClick={onOpenAccount} className="w-full sm:w-auto">
      {t("groupConnections.linkAccount")}
    </Button>
  ) : (
    <Button disabled={!canSelect} onClick={() => void select()} className="w-full sm:w-auto">
      {t(invitation ? "groupConnections.accept" : "groupConnections.invite")}
      <ExternalLink aria-hidden="true" className="h-3.5 w-3.5" />
    </Button>
  );
  return (
    <div className={fill ? "flex min-h-full min-w-0 flex-col gap-4" : "min-w-0 space-y-4"}>
      {invitation && (
        <p className="text-sm text-[var(--color-text-secondary)]">
          {t("groupConnections.invitation")}
        </p>
      )}
      <section
        aria-label={t("groupConnections.title")}
        className={fill ? "flex min-w-0 flex-1 flex-col justify-center" : "min-w-0"}
      >
        {links.length === 0 ? (
          <div className="flex flex-col items-center gap-3 py-6 text-center">
            <div
              aria-hidden="true"
              className="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]"
            >
              {failed ? <TriangleAlert className="h-6 w-6" /> : <Link2 className="h-6 w-6" />}
            </div>
            <div className="min-w-0 w-full max-w-sm space-y-4">
              <div
                className="space-y-1.5"
                role={failed ? "alert" : confirming ? "status" : undefined}
              >
                <h3 className="break-words text-base font-semibold leading-snug">
                  {unlinked ? t("groupConnections.linkTitle") : headline || error}
                </h3>
                {(unlinked || empty) && (
                  <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
                    {t(unlinked ? "groupConnections.linkHint" : "groupConnections.emptyHint")}
                  </p>
                )}
                {failed && value?.error_message && (
                  <p className="break-words text-xs text-[var(--color-text-secondary)]">
                    {value.error_message}
                  </p>
                )}
                {error && headline && (
                  <p className="break-words text-xs text-[var(--color-danger)]">{error}</p>
                )}
              </div>
              {primary}
            </div>
          </div>
        ) : (
          <div className="space-y-3 py-2">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p
                role={failed ? "alert" : confirming ? "status" : undefined}
                className="text-sm font-medium"
              >
                {headline}
              </p>
              {primary}
            </div>
            {error && (
              <p role="alert" className="text-xs text-[var(--color-danger)]">
                {error}
              </p>
            )}
            {failed && value?.error_message && (
              <p className="break-words text-xs text-[var(--color-text-secondary)]">
                {value.error_message}
              </p>
            )}
            <ul className="divide-y divide-[var(--glass-border-subtle)]">
              {links.map((link) => {
                const peer =
                  link.source.account_id === value?.account_id ? link.target : link.source;
                return (
                  <li
                    key={link.id}
                    className="flex flex-wrap items-start justify-between gap-3 py-3"
                  >
                    <div className="min-w-0 space-y-1">
                      <p className="break-words text-sm font-medium [overflow-wrap:anywhere]">
                        {peer.title}
                      </p>
                      <p className="break-words text-xs text-[var(--color-text-secondary)] [overflow-wrap:anywhere]">
                        {peer.instance.display_name}
                      </p>
                      <p className="text-xs text-[var(--color-text-secondary)]">
                        {t(
                          value?.direct_routes?.includes(link.id)
                            ? "direct.routeSelected"
                            : "groupConnections.connected",
                        )}
                      </p>
                      <code className="block break-all text-xs text-[var(--color-text-tertiary)]">
                        {peer.group_id}
                      </code>
                    </div>
                    {value?.account_origin && (
                      <a
                        href={accountLink(`/connect/${link.id}/disconnect`)}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="shrink-0 text-xs text-[var(--color-text-secondary)] underline-offset-2 hover:text-[var(--color-danger)] hover:underline"
                      >
                        {t("groupConnections.disconnect")}
                      </a>
                    )}
                  </li>
                );
              })}
            </ul>
          </div>
        )}
      </section>
      {prepared && (
        <a
          className="inline-block text-sm underline"
          href={prepared}
          target="_blank"
          rel="noopener noreferrer"
        >
          {t("groupConnections.continue")}
        </a>
      )}
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2 border-t border-[var(--glass-border-subtle)] pt-2">
        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
          <Button
            variant="ghost"
            size="sm"
            className="-ml-2 gap-1.5 px-2 text-xs"
            disabled={busy}
            onClick={refresh}
          >
            <RefreshCw aria-hidden="true" className="h-3.5 w-3.5" />
            {t("groupConnections.refresh")}
          </Button>
          {value?.checked_at && (
            <span className="text-xs text-[var(--color-text-tertiary)]">
              {t("groupConnections.checkedAt", {
                time: new Date(value.checked_at).toLocaleTimeString(),
              })}
            </span>
          )}
        </div>
        {!unlinked && value?.account_origin && (
          <a
            className="inline-flex items-center gap-1 text-xs text-[var(--color-text-secondary)] underline-offset-2 hover:underline"
            href={accountLink("/connect")}
            target="_blank"
            rel="noopener noreferrer"
          >
            {t("groupConnections.manage")}
            <ExternalLink aria-hidden="true" className="h-3 w-3" />
          </a>
        )}
      </div>
    </div>
  );
}
