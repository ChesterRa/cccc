import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiJson } from "../../../services/api/base";
import type { ConnectSnapshot } from "../../../features/connect/protocol";
import { ConnectInstanceName } from "./ConnectInstanceName";

// This panel reads daemon-owned confirmation. Opening Settings never registers
// an instance, starts a tunnel, or contacts the account/provider itself.
export function ConnectStatus({
  active,
  deviceId,
  refreshedAt,
}: {
  active: boolean;
  deviceId: string;
  refreshedAt: string;
}) {
  const { t, i18n } = useTranslation("settings");
  const [snapshot, setSnapshot] = useState<ConnectSnapshot | null>(null);
  const [failed, setFailed] = useState(false);
  const [nameRevision, setNameRevision] = useState(0);
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    let timer: number | undefined;
    const controller = new AbortController();
    const refresh = async () => {
      const result = await apiJson<{ connect: ConnectSnapshot | null }>("/api/v1/connect", {
        signal: AbortSignal.any([controller.signal, AbortSignal.timeout(5000)]),
      });
      if (cancelled) return;
      setFailed(!result.ok);
      setSnapshot(
        result.ok && result.result.connect?.device_id === deviceId ? result.result.connect : null,
      );
      timer = window.setTimeout(() => void refresh(), 15000);
    };
    void refresh();
    return () => {
      cancelled = true;
      controller.abort();
      window.clearTimeout(timer);
    };
  }, [active, deviceId, refreshedAt, nameRevision]);
  if (!active) return null;
  const directory =
    snapshot?.device_id === deviceId &&
    snapshot.directory &&
    Date.parse(snapshot.directory.expires_at) > Date.now()
      ? snapshot.directory
      : null;
  const own = directory?.instances.find(
    (instance) => instance.instance_id === snapshot?.instance_id,
  );
  const key = failed
    ? "unavailable"
    : directory
      ? own?.public_origin
        ? "confirmed"
        : "noRoute"
      : snapshot?.error_code === "membership_unsupported_version"
        ? "upgrade"
        : snapshot?.error_code
          ? "unavailable"
          : "pending";
  const checked = snapshot?.checked_at ? new Date(snapshot.checked_at) : null;
  return (
    <div className="mt-3 space-y-1 text-xs leading-5" data-testid="connect-status">
      {own ? (
        <ConnectInstanceName
          key={deviceId}
          name={own.display_name}
          origin={own.public_origin}
          onSaved={(name) => {
            setSnapshot((previous) =>
              previous?.directory
                ? {
                    ...previous,
                    directory: {
                      ...previous.directory,
                      instances: previous.directory.instances.map((instance) =>
                        instance.device_id === deviceId
                          ? { ...instance, display_name: name }
                          : instance,
                      ),
                    },
                  }
                : previous,
            );
            setNameRevision((previous) => previous + 1);
          }}
        />
      ) : null}
      <p className="font-medium text-[var(--color-text-primary)]" role="status">
        {t(`account.connect.${key}`)}
      </p>
      {own ? (
        <p className="text-[var(--color-text-muted)]">{t("account.connect.adminSetupHint")}</p>
      ) : null}
      {directory ? (
        <p className="text-[var(--color-text-muted)]">
          {t("account.connect.peerCount", { count: Math.max(0, directory.instances.length - 1) })}
        </p>
      ) : null}
      {snapshot?.error_message ? (
        <p className="text-[var(--color-text-secondary)]">{snapshot.error_message}</p>
      ) : null}
      {checked && Number.isFinite(checked.getTime()) ? (
        <p className="text-[var(--color-text-muted)]">
          {t("account.connect.checkedAt", {
            time: checked.toLocaleTimeString(i18n.resolvedLanguage || i18n.language),
          })}
        </p>
      ) : null}
    </div>
  );
}
