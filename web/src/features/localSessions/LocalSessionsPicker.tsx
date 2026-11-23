import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { useUIStore } from "../../stores";
import { LocalSessionsDefaults } from "./LocalSessionsDefaults";
import { LocalSessionsSelection } from "./LocalSessionsSelection";
import { isDeviceSessionSource } from "./remoteSessionPolicy";
import { SessionNodesPanel } from "./SessionNodesPanel";
import { useLocalSessionsConnection } from "./useLocalSessionsConnection";
import { useSessionNodes } from "./useSessionNodes";

export function LocalSessionsPicker({
  active,
  groupId,
  isDark,
  onClose,
  onSelectGroup,
  onBusyChange,
  onPairingChange,
}: {
  active: boolean;
  groupId: string | null;
  isDark: boolean;
  onClose: () => void;
  onSelectGroup: (id: string) => void;
  onBusyChange: (busy: boolean) => void;
  onPairingChange?: (pairing: boolean) => void;
}) {
  const { t } = useTranslation("layout");
  const [pairing, setPairing] = useState(false);
  const [visitedPairing, setVisitedPairing] = useState(false);
  const [title, setTitle] = useState("");
  const scroller = useRef<HTMLDivElement>(null);
  const pairingPanel = useRef<HTMLDivElement>(null);
  const pairButton = useRef<HTMLButtonElement>(null);
  const otherHeading = useRef<HTMLHeadingElement>(null);
  const scrollToRestore = useRef<number | null>(null);
  const listVisible = active && !pairing;
  const { directory: deviceDirectory } = useSessionNodes(listVisible);
  const connection = useLocalSessionsConnection({
    active: listVisible,
    groupId,
    devices: deviceDirectory.status === "ready" ? deviceDirectory.devices : undefined,
    onClose,
    onSelectGroup,
  });
  const creating = groupId === null;
  const { admitting, busy, directory, selected, targetUnavailable } = connection;
  const localSelected = selected.filter((source) => !isDeviceSessionSource(source));
  const count = selected.length;

  useEffect(() => {
    onBusyChange(busy);
  }, [busy, onBusyChange]);

  useEffect(() => {
    onPairingChange?.(pairing);
  }, [onPairingChange, pairing]);

  useEffect(() => {
    if (active && targetUnavailable) {
      useUIStore.getState().showNotice({ message: t("localSessions.targetUnavailable") });
      onClose();
    }
  }, [active, onClose, t, targetUnavailable]);

  useEffect(() => {
    if (active && pairing) pairingPanel.current?.querySelector("button")?.focus();
  }, [active, pairing]);

  const listRestored =
    listVisible &&
    !directory.loading &&
    !directory.deviceLoading &&
    deviceDirectory.status !== "loading" &&
    deviceDirectory.status !== "rechecking";
  useEffect(() => {
    if (!listRestored || scrollToRestore.current === null) return;
    if (scroller.current) scroller.current.scrollTop = scrollToRestore.current;
    scrollToRestore.current = null;
  }, [listRestored]);

  const openPairing = () => {
    scrollToRestore.current = scroller.current?.scrollTop ?? null;
    setVisitedPairing(true);
    setPairing(true);
  };

  const backToSessions = () => {
    setPairing(false);
    requestAnimationFrame(() => {
      if (scrollToRestore.current !== null && scroller.current)
        scroller.current.scrollTop = scrollToRestore.current;
      (pairButton.current ?? otherHeading.current)?.focus({ preventScroll: true });
    });
  };

  return (
    <>
      <div hidden={pairing} className={pairing ? undefined : "flex min-h-0 flex-1 flex-col"}>
        <div
          ref={scroller}
          className="min-h-0 flex-1 space-y-4 overflow-y-auto bg-[var(--color-bg-primary)] p-4 sm:p-6"
        >
          {creating && (
            <div className="grid min-w-0 gap-3 sm:grid-cols-2">
              <label className="min-w-0 space-y-1 text-sm">
                <span>{t("modals:createGroup.groupName")}</span>
                <Input
                  value={title}
                  onChange={(event) => setTitle(event.target.value)}
                  disabled={busy}
                  placeholder={t("modals:createGroup.sessionGroupNamePlaceholder")}
                />
              </label>
              <div className="min-w-0 space-y-1 text-sm">
                <div>{t("modals:createGroup.projectDirectory")}</div>
                <div className="min-h-10 break-all rounded-lg border border-[var(--glass-border)] p-2 font-mono text-[var(--color-text-secondary)]">
                  {localSelected[0]?.cwd ||
                    t(
                      count
                        ? "modals:createGroup.noLocalProjectDirectory"
                        : "localSessions.chooseDirectorySource",
                    )}
                </div>
                {!!localSelected.length && (
                  <p className="text-xs text-[var(--color-text-muted)]">
                    {t("localSessions.foremanDirectoryHint")}
                  </p>
                )}
              </div>
            </div>
          )}
          <LocalSessionsSelection
            connection={connection}
            deviceDirectory={deviceDirectory}
            isDark={isDark}
            onPair={openPairing}
            pairButton={pairButton}
            otherHeading={otherHeading}
          />
          {creating && (
            <LocalSessionsDefaults
              selected={selected}
              setSelected={connection.setSelected}
              busy={busy}
            />
          )}
        </div>
        <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-[var(--glass-border-subtle)] glass-header px-4 pt-3 pb-[calc(0.75rem+env(safe-area-inset-bottom,0px))] sm:px-6">
          <div className="min-w-0 text-sm text-[var(--color-text-secondary)]">
            {creating ? (
              <p>{t("modals:createGroup.usingSelectedSessions", { count })}</p>
            ) : (
              <>
                <p className="break-words font-medium">
                  {t("localSessions.targetGroupSummary", {
                    group: connection.target?.title || groupId,
                  })}
                </p>
                <p>
                  {t("localSessions.selectedCountByComputer", {
                    count,
                    localCount: localSelected.length,
                    remoteCount: count - localSelected.length,
                  })}
                </p>
              </>
            )}
          </div>
          <div className="ml-auto flex flex-wrap gap-2">
            <Button variant="secondary" onClick={onClose}>
              {t("common:cancel")}
            </Button>
            <Button
              onClick={() => void connection.start(title)}
              disabled={!connection.canSubmit}
              aria-busy={admitting || undefined}
            >
              {creating
                ? t(admitting ? "modals:createGroup.creating" : "modals:createGroup.createGroup")
                : admitting
                  ? t("localSessions.joining")
                  : count
                    ? t("localSessions.attachSessions", { count })
                    : t("localSessions.attachSelectedSessions")}
            </Button>
          </div>
        </div>
      </div>
      {visitedPairing && (
        <div
          ref={pairingPanel}
          hidden={!pairing}
          className="min-h-0 flex-1 overflow-y-auto bg-[var(--color-bg-primary)] p-4 sm:p-6"
          onKeyDown={(event) => {
            if (event.key !== "Escape" || event.defaultPrevented) return;
            event.preventDefault();
            backToSessions();
          }}
        >
          {!!count && (
            <p className="mb-2 text-sm text-[var(--color-text-muted)]">
              {t("localSessions.pairSelectionKept", { count })}
            </p>
          )}
          <SessionNodesPanel
            mode="picker"
            active={active && pairing}
            onBackToSessions={backToSessions}
          />
        </div>
      )}
    </>
  );
}
