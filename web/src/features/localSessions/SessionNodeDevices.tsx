import { useId } from "react";
import { Laptop } from "lucide-react";
import { useTranslation } from "react-i18next";
import { LcdIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import type { SessionNodeDevice } from "../../services/api/sessionNodes";

export function SessionNodeDevices({
  devices,
  busy,
  revokeId,
  setRevokeId,
  revoke,
  onChooseSessions,
}: {
  devices: SessionNodeDevice[];
  busy: boolean;
  revokeId: string;
  setRevokeId: (id: string) => void;
  revoke: () => Promise<void>;
  onChooseSessions: (device: SessionNodeDevice) => void;
}) {
  const { t } = useTranslation("layout");
  const listId = useId();
  return (
    <ul
      aria-label={t("sessionNodes.devices")}
      className="divide-y divide-[var(--glass-border-subtle)]"
    >
      {devices.map((device, index) => {
        const identity = `${listId}-${index}`;
        const online = device.enabled === true && device.connected === true;
        const confirming = device.enabled && revokeId === device.node_id;
        const shortId =
          device.node_id.length > 10 ? "…" + device.node_id.slice(-10) : device.node_id;
        return (
          <li key={device.node_id} className="min-w-0 space-y-2.5 py-3">
            <div className="flex min-w-0 items-start gap-2.5">
              <LcdIcon
                icon={Laptop}
                glyph="server"
                size={18}
                aria-hidden="true"
                className="mt-0.5 shrink-0 text-[var(--color-text-secondary)]"
              />
              <div id={identity} className="min-w-0 flex-1">
                <p dir="auto" className="break-words text-sm font-semibold">
                  {device.name}
                </p>
                <p className="text-xs text-[var(--color-text-secondary)]">
                  {t("sessionNodes.deviceId")}{" "}
                  <bdi dir="ltr" title={device.node_id} className="font-mono">
                    {shortId}
                  </bdi>
                </p>
              </div>
              <span className="inline-flex shrink-0 items-center gap-1.5 text-xs text-[var(--color-text-secondary)]">
                <span
                  aria-hidden="true"
                  className={`h-1.5 w-1.5 rounded-full ${online ? "bg-[var(--color-accent-success)]" : device.enabled ? "bg-[var(--color-text-tertiary)]" : "border border-[var(--color-text-tertiary)]"}`}
                />
                {t(
                  !device.enabled
                    ? "sessionNodes.revoked"
                    : online
                      ? "sessionNodes.online"
                      : "sessionNodes.offline",
                )}
              </span>
            </div>
            {confirming ? (
              <div className="space-y-2 rounded-lg border border-[var(--color-border-primary)] p-2.5">
                <p className="text-sm">{t("sessionNodes.revokeConfirm")}</p>
                <p
                  className="break-all font-mono text-xs text-[var(--color-text-secondary)]"
                  dir="ltr"
                >
                  {device.node_id}
                </p>
                <div className="flex flex-wrap gap-2">
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    className="max-sm:min-h-11"
                    disabled={busy}
                    onClick={() => setRevokeId("")}
                  >
                    {t("common:cancel")}
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant="destructive"
                    className="max-sm:min-h-11"
                    disabled={busy}
                    onClick={() => void revoke()}
                  >
                    {t("sessionNodes.revoke")}
                  </Button>
                </div>
              </div>
            ) : (
              device.enabled && (
                <div className="flex flex-wrap gap-1">
                  {online && (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="max-sm:min-h-11"
                      aria-describedby={identity}
                      disabled={busy}
                      onClick={() => onChooseSessions(device)}
                    >
                      {t("sessionNodes.chooseSessionsToJoin")}
                    </Button>
                  )}
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="max-sm:min-h-11 text-[var(--color-text-secondary)]"
                    disabled={busy}
                    aria-describedby={identity}
                    onClick={() => setRevokeId(device.node_id)}
                  >
                    {t("sessionNodes.revoke")}
                  </Button>
                </div>
              )
            )}
          </li>
        );
      })}
    </ul>
  );
}
