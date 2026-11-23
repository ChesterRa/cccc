import { useRef, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { LocalSessionsHelp } from "./LocalSessionsHelp";
import { LocalSessionsList } from "./LocalSessionsList";
import { isDeviceSessionSource } from "./remoteSessionPolicy";
import type { useLocalSessionsConnection } from "./useLocalSessionsConnection";
import type { useSessionNodes } from "./useSessionNodes";

export function LocalSessionsSelection({
  connection,
  deviceDirectory,
  isDark,
  onPair,
  pairButton,
  otherHeading,
}: {
  connection: ReturnType<typeof useLocalSessionsConnection>;
  deviceDirectory: ReturnType<typeof useSessionNodes>["directory"];
  isDark: boolean;
  onPair: () => void;
  pairButton: RefObject<HTMLButtonElement | null>;
  otherHeading: RefObject<HTMLHeadingElement | null>;
}) {
  const { t } = useTranslation("layout");
  const ownHeading = useRef<HTMLHeadingElement>(null);
  const { directory, selected, members } = connection;
  const localSources = directory.sources.filter((source) => !isDeviceSessionSource(source));
  const localSelected = selected.filter((source) => !isDeviceSessionSource(source));
  const localRows = directory.error
    ? localSelected.map((source) => ({ ...source, connected: false }))
    : [
        ...localSources,
        ...localSelected.filter(
          (source) => !localSources.some((row) => row.source_id === source.source_id),
        ),
      ];
  const devices =
    deviceDirectory.status === "ready"
      ? deviceDirectory.devices.filter((device) => device.enabled)
      : [];
  const deviceSources = directory.sources.filter(isDeviceSessionSource);
  const online = new Set(
    devices.filter((device) => device.connected).map((device) => device.node_id),
  );
  const remoteReadable =
    deviceDirectory.status === "ready" && !directory.deviceError && !directory.deviceLoading;
  const unavailableSelected = selected.filter(
    (source) =>
      isDeviceSessionSource(source) &&
      (!remoteReadable ||
        !online.has(source.node_id) ||
        !deviceSources.some((row) => row.source_id === source.source_id && row.connected)),
  );
  const unavailableIds = new Set(unavailableSelected.map((source) => source.source_id));
  const nameCounts = new Map<string, number>();
  for (const device of devices) nameCounts.set(device.name, (nameCounts.get(device.name) ?? 0) + 1);
  const removed = directory.removed_sources?.length || 0;
  const allInGroup =
    localRows.length > 0 && localRows.every((source) => members.has(source.source_id));
  const unpaired = remoteReadable && devices.length === 0;
  const jump = (ref: RefObject<HTMLHeadingElement | null>) => {
    ref.current?.focus({ preventScroll: true });
    ref.current?.scrollIntoView?.({ block: "start" });
  };

  return (
    <section className="space-y-4" aria-label={t("localSessions.sessionsHeading")}>
      <p className="text-sm text-[var(--color-text-secondary)]">
        {t(connection.target ? "localSessions.joinHint" : "localSessions.hint")}
      </p>
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" onClick={() => jump(ownHeading)}>
          {t("localSessions.thisDevice")}
        </Button>
        <Button variant="outline" size="sm" onClick={() => jump(otherHeading)}>
          {t("localSessions.otherComputers")}
          {unpaired ? ` · ${t("sessionNodes.unpaired")}` : ""}
          <span className="sm:hidden" aria-hidden="true">
            ↓
          </span>
        </Button>
      </div>
      {connection.notice && <p role="status">{connection.notice}</p>}
      {connection.membersFailed && <p role="alert">{t("localSessions.membersFailed")}</p>}
      {connection.target && !connection.membersReady && !connection.membersFailed && (
        <p role="status" className="text-sm text-[var(--color-text-secondary)]">
          {t("localSessions.membersLoading")}
        </p>
      )}
      {connection.unavailableCount > 0 && (
        <p
          role="alert"
          className="text-sm text-amber-700 dark:text-amber-300"
          data-status-tone="warning"
        >
          {t("localSessions.selectionUnavailable", { count: connection.unavailableCount })}
        </p>
      )}
      <section
        aria-label={t("localSessions.thisDevice")}
        className="min-w-0 rounded-xl border border-[var(--glass-border)]"
      >
        <h3
          ref={ownHeading}
          tabIndex={-1}
          className="border-b border-[var(--glass-border)] px-3 py-2 text-sm font-semibold"
        >
          {t("localSessions.thisDevice")}
        </h3>
        {connection.management.error && (
          <p
            role="alert"
            className="break-words px-3 py-2 text-sm text-[var(--color-accent-danger)]"
          >
            {connection.management.error}
          </p>
        )}
        {connection.management.notice && (
          <p role="status" className="break-words px-3 py-2 text-sm">
            {connection.management.notice}
          </p>
        )}
        {directory.loading ? (
          <p role="status" aria-busy="true" className="p-3 text-sm">
            {t("common:loading")}
          </p>
        ) : directory.error ? (
          <p role="alert" className="break-words p-3 text-sm text-[var(--color-accent-danger)]">
            {t("localSessions.loadFailed")}
            <span className="block">{directory.error}</span>
          </p>
        ) : !localRows.length && !removed ? (
          <div className="space-y-2 p-3">
            <p role="status" className="text-sm">
              {t("localSessions.empty")}
            </p>
            <LocalSessionsHelp defaultSetupOpen />
          </div>
        ) : allInGroup ? (
          <p className="px-3 pt-2 text-sm text-[var(--color-text-secondary)]">
            {t("localSessions.allMembersJoined")}
          </p>
        ) : null}
        <LocalSessionsList connection={connection} sources={localRows} isDark={isDark} />
        {!!removed && (
          <p className="border-t border-[var(--glass-border)] px-3 py-2 text-sm text-[var(--color-text-secondary)]">
            {t("localSessions.removedInSettings", { count: removed })}
          </p>
        )}
        {!!localRows.length && !directory.loading && !directory.error && (
          <div className="border-t border-[var(--glass-border)] px-3 py-2">
            <LocalSessionsHelp />
          </div>
        )}
      </section>

      <section
        aria-label={t("localSessions.otherComputers")}
        className="min-w-0 rounded-xl border border-[var(--glass-border)]"
      >
        <header className="flex items-center justify-between gap-2 border-b border-[var(--glass-border)] px-3 py-2">
          <h3 ref={otherHeading} tabIndex={-1} className="min-w-0 text-sm font-semibold">
            {t("localSessions.otherComputers")}
          </h3>
          <Button
            ref={pairButton}
            variant="outline"
            size="sm"
            disabled={connection.busy}
            onClick={onPair}
          >
            {t("localSessions.addDevice")}
          </Button>
        </header>
        {unpaired && (
          <p role="status" className="px-3 py-2 text-sm">
            {t("sessionNodes.pairToChooseSessions")}
          </p>
        )}
        {(deviceDirectory.status === "loading" ||
          deviceDirectory.status === "rechecking" ||
          (deviceDirectory.status === "ready" && directory.deviceLoading)) && (
          <p role="status" aria-busy="true" className="px-3 py-2 text-sm">
            {t("common:loading")}
          </p>
        )}
        {deviceDirectory.status === "unsupported" && (
          <p role="status" className="px-3 py-2 text-sm">
            {t("sessionNodes.upgradeRequired")}
          </p>
        )}
        {deviceDirectory.status === "forbidden" && (
          <p role="alert" className="px-3 py-2 text-sm">
            {t("sessionNodes.adminRequired")}
          </p>
        )}
        {deviceDirectory.status === "failed" && (
          <p role="alert" className="px-3 py-2 text-sm">
            {t("sessionNodes.directoryFailed")}
          </p>
        )}
        {directory.deviceError && (
          <p
            role="alert"
            className="break-words px-3 py-2 text-sm text-[var(--color-accent-danger)]"
          >
            {t("sessionNodes.deviceSourcesFailed")}
            <span className="block">{directory.deviceError}</span>
          </p>
        )}
        {remoteReadable &&
          devices.map((device) => {
            const label =
              (nameCounts.get(device.name) || 0) > 1
                ? `${device.name} · ${device.node_id.slice(0, 8)}`
                : device.name;
            const rows = device.connected
              ? deviceSources.filter(
                  (source) =>
                    source.node_id === device.node_id && !unavailableIds.has(source.source_id),
                )
              : [];
            return (
              <section
                key={device.node_id}
                aria-label={label}
                className="border-t border-[var(--glass-border)]"
              >
                <h4 className="flex items-center gap-2 px-3 py-2 text-sm font-medium">
                  <span className="truncate" title={label}>
                    {label}
                  </span>
                  {!device.connected && (
                    <span className="text-xs text-[var(--color-text-muted)]">
                      {t("sessionNodes.offline")}
                    </span>
                  )}
                </h4>
                {device.connected && !rows.some((source) => source.connected) && (
                  <p className="px-3 pb-2 text-sm text-[var(--color-text-secondary)]">
                    {t("sessionNodes.onlineWithoutSessions")}
                  </p>
                )}
                <LocalSessionsList connection={connection} sources={rows} isDark={isDark} />
              </section>
            );
          })}
        {!!unavailableSelected.length && (
          <section
            aria-label={t("localSessions.unavailableSelected")}
            className="border-t border-[var(--glass-border)]"
          >
            <h4 className="px-3 py-2 text-sm font-medium">
              {t("localSessions.unavailableSelected")}
            </h4>
            <LocalSessionsList
              connection={connection}
              sources={unavailableSelected.map((source) => ({ ...source, connected: false }))}
              isDark={isDark}
            />
          </section>
        )}
      </section>
    </section>
  );
}
