import { ChevronRight, Clock3, Laptop } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import type {
  SessionNodeDevice,
  SessionNodeJoinDetail,
  SessionNodeJoinRequest,
} from "../../services/api/sessionNodes";
import { SessionCommand } from "./SessionCommand";
import { SessionNodeProgress } from "./SessionNodeProgress";
import { sessionNodeJoinCommand } from "./sessionNodeJoin";
import type { SessionNodeJoinDirectory } from "./useSessionNodes";

type Row = Pick<SessionNodeJoinRequest, "request_id" | "name" | "user_code">;

export function SessionNodeJoinPreparation({
  join,
  handled,
  devices,
  onReview,
}: {
  join: SessionNodeJoinDirectory;
  handled: SessionNodeJoinDetail[];
  devices: SessionNodeDevice[];
  onReview: (id: string) => void;
}) {
  const { t } = useTranslation("layout");
  const command = sessionNodeJoinCommand(join.url, window.location.origin);
  const rows = join.pending?.filter((r) => !handled.some((h) => h.request_id === r.request_id));
  const hasPending = !!rows?.length;
  const hasRequests = hasPending || handled.length > 0;
  const approved = handled.filter((item) => item.status === "approved");
  const allConnected =
    approved.length > 0 &&
    approved.every((item) =>
      devices.some(
        (device) =>
          device.node_id === item.node_id && device.enabled === true && device.connected === true,
      ),
    );
  function row(request: Row, result?: string) {
    return (
      <li
        key={request.request_id}
        className="flex min-w-0 flex-wrap items-center gap-2 border-b border-[var(--glass-border-subtle)] py-2.5 last:border-b-0"
      >
        <Laptop
          size={18}
          aria-hidden="true"
          className="shrink-0 text-[var(--color-text-secondary)]"
        />
        <div className="min-w-0 flex-1">
          <p dir="auto" className="break-words font-medium">
            {request.name}
          </p>
          <p className="text-xs text-[var(--color-text-secondary)]">
            {t("sessionNodes.joinCode")}{" "}
            <code
              dir="ltr"
              className="whitespace-nowrap tracking-wider text-[var(--color-text-primary)]"
            >
              {request.user_code}
            </code>
          </p>
          {result && <p className="text-xs text-[var(--color-text-secondary)]">{result}</p>}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="min-h-10 max-sm:min-h-11"
          onClick={() => onReview(request.request_id)}
          aria-label={t(result ? "sessionNodes.joinViewDevice" : "sessionNodes.joinReviewDevice", {
            name: request.name,
            code: request.user_code,
          })}
        >
          {t(result ? "sessionNodes.joinView" : "sessionNodes.joinReview")}
          <ChevronRight size={14} aria-hidden="true" />
        </Button>
      </li>
    );
  }
  const showCommand = command && (
    <SessionCommand
      key={command}
      command={command}
      copyLabel={t("sessionNodes.joinCopy")}
      emphasis={hasRequests ? "secondary" : "primary"}
    />
  );
  return (
    <>
      <SessionNodeProgress
        step={hasPending ? 2 : allConnected ? 4 : approved.length ? 3 : hasRequests ? 2 : 1}
      />
      <div className="space-y-4">
        {!hasRequests && command && (
          <section className="space-y-2">
            <h3 className="text-base font-semibold">{t("sessionNodes.joinCommandTitle")}</h3>
            <p className="leading-relaxed text-[var(--color-text-secondary)]">
              {t("sessionNodes.joinCommandHint")}
            </p>
            {showCommand}
          </section>
        )}
        {hasPending && (
          <section>
            <h3 className="text-base font-semibold">{t("sessionNodes.joinPending")}</h3>
            <p className="mt-1 text-[var(--color-text-secondary)]">
              {t("sessionNodes.joinQueueHint")}
            </p>
            <ul>{rows!.map((r) => row(r))}</ul>
          </section>
        )}
        {handled.length > 0 && (
          <section>
            <h3 className="text-sm font-semibold">{t("sessionNodes.joinThisSetup")}</h3>
            <ul>
              {handled.map((r) => {
                const device = devices.find((d) => d.node_id === r.node_id);
                const label =
                  r.status === "approved"
                    ? device && !device.enabled
                      ? "revoked"
                      : device?.enabled === true && device.connected === true
                        ? "online"
                        : "joinApproved"
                    : r.status === "denied"
                      ? "joinDenied"
                      : "joinExpired";
                return row(r, t(`sessionNodes.${label}`));
              })}
            </ul>
          </section>
        )}
        {!command && (
          <p role="status" className="text-sm text-[var(--color-text-secondary)]">
            {t("sessionNodes.joinNoAddress")}
          </p>
        )}
        {join.error ? (
          <p role="alert">
            {t(
              join.error === "forbidden"
                ? "sessionNodes.joinForbidden"
                : "sessionNodes.joinReadFailed",
            )}
          </p>
        ) : rows === undefined ? (
          <p role="status">{t("common:loading")}</p>
        ) : (
          !hasRequests &&
          command && (
            <div
              role="status"
              className="flex gap-2.5 border-t border-[var(--glass-border-subtle)] pt-3"
            >
              <Clock3
                aria-hidden="true"
                size={17}
                className="mt-0.5 shrink-0 text-[var(--color-text-secondary)]"
              />
              <div>
                <p className="font-medium">{t("sessionNodes.joinWaiting")}</p>
                <p className="text-xs leading-relaxed text-[var(--color-text-secondary)]">
                  {t("sessionNodes.joinNone")}
                </p>
              </div>
            </div>
          )
        )}
        {hasRequests && showCommand && (
          <details>
            <summary className="cursor-pointer py-2 text-sm text-[var(--color-text-secondary)]">
              {t("sessionNodes.joinShowCommand")}
            </summary>
            {showCommand}
          </details>
        )}
      </div>
    </>
  );
}
