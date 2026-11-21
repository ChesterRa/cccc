import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { SessionCommand } from "./SessionCommand";
import { sessionNodeJoinCommand } from "./sessionNodeJoin";
import type { SessionNodeJoinDirectory } from "./useSessionNodes";

export function SessionNodeJoinPreparation({
  join,
  onReview,
}: {
  join: SessionNodeJoinDirectory;
  onReview: (id: string) => void;
}) {
  const { t } = useTranslation("layout");
  const command = sessionNodeJoinCommand(join.url, window.location.origin);
  return (
    <>
      <section className="min-w-0 space-y-3 border border-[var(--glass-border-subtle)] p-4 sm:p-5">
        <h4 className="text-sm font-semibold">{t("sessionNodes.joinCommandTitle")}</h4>
        {command ? (
          <>
            <SessionCommand
              key={command}
              command={command}
              copyLabel={t("sessionNodes.joinCopy")}
            />
            <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
              {t("sessionNodes.joinCommandHint")}
            </p>
          </>
        ) : (
          <p role="status" className="text-sm leading-relaxed">
            {t("sessionNodes.joinNoAddress")}
          </p>
        )}
      </section>
      <section className="min-w-0 space-y-3 border border-[var(--glass-border-subtle)] p-4 sm:p-5">
        <h4 className="text-sm font-semibold">{t("sessionNodes.joinPending")}</h4>
        {join.error ? (
          <p role="alert">
            {t(
              join.error === "forbidden"
                ? "sessionNodes.joinForbidden"
                : "sessionNodes.joinReadFailed",
            )}
          </p>
        ) : join.pending === null ? (
          <p role="status">{t("common:loading")}</p>
        ) : join.pending.length === 0 ? (
          <p className="text-sm text-[var(--color-text-secondary)]">{t("sessionNodes.joinNone")}</p>
        ) : (
          <ul className="divide-y divide-[var(--glass-border-subtle)]">
            {join.pending.map((request) => (
              <li
                key={request.request_id}
                className="flex min-w-0 items-center justify-between gap-2 py-2"
              >
                <div className="min-w-0 space-y-1">
                  <p dir="auto" className="break-words font-medium">
                    {request.name}
                  </p>
                  <p className="text-sm text-[var(--color-text-secondary)]">
                    {t("sessionNodes.joinCode")}{" "}
                    <code dir="ltr" className="font-mono tracking-wide">
                      {request.user_code}
                    </code>
                  </p>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  className="min-h-11 shrink-0"
                  onClick={() => onReview(request.request_id)}
                  aria-label={t("sessionNodes.joinReviewDevice", {
                    name: request.name,
                    code: request.user_code,
                  })}
                >
                  {t("sessionNodes.joinReview")}
                </Button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </>
  );
}
