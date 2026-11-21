import { ArrowRight, CircleCheck, Clock3 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import type { SessionNodeDevice, SessionNodeJoinDetail } from "../../services/api/sessionNodes";
import { SessionNodeProgress } from "./SessionNodeProgress";
import { useSessionNodes } from "./useSessionNodes";

export type DeviceDirectory = Pick<
  ReturnType<typeof useSessionNodes>,
  "devices" | "directoryStatus"
>;

type Props = {
  request: SessionNodeJoinDetail;
  directory: DeviceDirectory;
  onChooseSessions: () => void;
};

export function SessionNodeIdentity({ request }: { request: SessionNodeJoinDetail }) {
  const { t } = useTranslation("layout");
  return (
    <dl className="flex min-w-0 flex-wrap items-center justify-between gap-x-4 gap-y-2 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-2.5">
      <div className="min-w-0 flex-1">
        <dt className="text-xs text-[var(--color-text-secondary)]">
          {t("sessionNodes.joinDeviceName")}
        </dt>
        <dd dir="auto" className="break-words font-medium">
          {request.name}
        </dd>
      </div>
      {request.user_code && (
        <div>
          <dt className="text-xs text-[var(--color-text-secondary)]">
            {t("sessionNodes.joinCode")}
          </dt>
          <dd>
            <code dir="ltr" className="whitespace-nowrap text-lg font-semibold tracking-wider">
              {request.user_code}
            </code>
          </dd>
        </div>
      )}
    </dl>
  );
}

export function SessionNodeApprovalStatus({ request, directory, onChooseSessions }: Props) {
  const { t } = useTranslation("layout");
  const device: SessionNodeDevice | undefined = directory.devices.find(
    (d) => d.node_id === request.node_id,
  );
  const verified = directory.directoryStatus === "ready";
  const online = verified && device?.enabled === true && device.connected === true;
  const revoked = verified && device?.enabled === false;
  const status =
    directory.directoryStatus === "forbidden"
      ? "joinForbidden"
      : revoked
        ? "revoked"
        : online
          ? "joinOnline"
          : "joinApproved";
  return (
    <>
      <SessionNodeProgress step={online ? 4 : 3} />
      <div className="space-y-3" aria-live="polite">
        <div className="flex items-start gap-2.5">
          {online ? (
            <CircleCheck size={20} aria-hidden="true" className="mt-0.5" />
          ) : (
            <Clock3
              size={18}
              aria-hidden="true"
              className="mt-0.5 shrink-0 text-[var(--color-text-secondary)]"
            />
          )}
          <div>
            <h3 className="text-base font-semibold">{t(`sessionNodes.${status}`)}</h3>
            {directory.directoryStatus !== "forbidden" && !revoked && (
              <p className="mt-1 text-sm leading-relaxed text-[var(--color-text-secondary)]">
                {t(
                  online
                    ? "sessionNodes.joinOnlineHint"
                    : directory.directoryStatus === "failed"
                      ? "sessionNodes.joinConnectionUnknown"
                      : "sessionNodes.joinWaitHint",
                )}
              </p>
            )}
          </div>
        </div>
        {directory.directoryStatus !== "forbidden" && <SessionNodeIdentity request={request} />}
        {online && (
          <Button type="button" onClick={onChooseSessions}>
            {t("sessionNodes.viewSessions")}
            <ArrowRight aria-hidden="true" size={15} />
          </Button>
        )}
      </div>
    </>
  );
}

// A standalone administrator link only reads the device directory after this request is approved.
export function LinkedSessionNodeStatus({ request, onChooseSessions }: Omit<Props, "directory">) {
  const directory = useSessionNodes(false);
  return (
    <SessionNodeApprovalStatus
      request={request}
      directory={directory}
      onChooseSessions={onChooseSessions}
    />
  );
}
