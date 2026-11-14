import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import type { SessionNodeDevice } from "../../services/api/sessionNodes";

export function SessionNodeDevices({
  devices,
  busy,
  revokeId,
  setRevokeId,
  revoke,
}: {
  devices: SessionNodeDevice[];
  busy: boolean;
  revokeId: string;
  setRevokeId: (id: string) => void;
  revoke: () => Promise<void>;
}) {
  const { t } = useTranslation("layout");
  if (!devices.length) return null;
  return (
    <section className="space-y-2 border-t border-[var(--glass-border-subtle)] pt-4">
      <h4 className="text-sm font-medium">
        {t("sessionNodes.devices")}{" "}
        <span className="text-[var(--color-text-secondary)]">{devices.length}</span>
      </h4>
      <ul className="divide-y divide-[var(--glass-border-subtle)]">
        {devices.map((device) => (
          <li
            key={device.node_id}
            className="flex min-w-0 flex-wrap items-center justify-between gap-2 py-2"
          >
            <span className="min-w-0 flex-1 break-words">
              <span className="font-medium">{device.name}</span>
              <span className="ml-2 text-sm text-[var(--color-text-secondary)]">
                {t(
                  !device.enabled
                    ? "sessionNodes.revoked"
                    : device.connected
                      ? "sessionNodes.online"
                      : "sessionNodes.offline",
                )}
              </span>
            </span>
            {device.enabled &&
              (revokeId === device.node_id ? (
                <div className="flex basis-full flex-wrap items-center justify-end gap-2">
                  <span>{t("sessionNodes.revokeConfirm")}</span>
                  <Button
                    variant="outline"
                    className="min-h-11 text-sm"
                    disabled={busy}
                    onClick={() => setRevokeId("")}
                  >
                    {t("common:cancel")}
                  </Button>
                  <Button
                    variant="destructive"
                    className="min-h-11 text-sm"
                    disabled={busy}
                    onClick={() => void revoke()}
                  >
                    {t("sessionNodes.revoke")}
                  </Button>
                </div>
              ) : (
                <Button
                  variant="ghost"
                  className="min-h-11 shrink-0"
                  disabled={busy}
                  onClick={() => setRevokeId(device.node_id)}
                >
                  {t("sessionNodes.revoke")}
                </Button>
              ))}
          </li>
        ))}
      </ul>
    </section>
  );
}
