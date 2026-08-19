import { useState } from "react";
import { useTranslation } from "react-i18next";

import { copyTextToClipboard } from "../../../utils/copy";
import {
  hostnameLooksTokenless,
  membershipCopyRows,
  membershipPanelKind,
  type MembershipCopyRowId,
  type MembershipState,
} from "./reachMembershipModel";
import { primaryButtonClass, secondaryButtonClass } from "./types";

interface ReachMembershipSectionProps {
  isDark: boolean;
  membership: MembershipState | null;
  reachBusy: boolean;
  onReachOn: () => void;
  onReachOff: () => void;
  onCopied: () => void;
  onCopyFailed: () => void;
}

export function ReachMembershipSection({
  membership,
  reachBusy,
  onReachOn,
  onReachOff,
  onCopied,
  onCopyFailed,
}: ReachMembershipSectionProps) {
  const { t } = useTranslation("settings");
  const [copiedId, setCopiedId] = useState<MembershipCopyRowId | null>(null);
  const kind = membershipPanelKind(membership);
  const rows = membershipCopyRows(membership);
  const hostname = String(membership?.hostname || "").trim();
  const hostnameSafe = !hostname || hostnameLooksTokenless(hostname);
  const canStart = kind === "offline";
  const canStop = kind === "online";

  const statusLabel =
    kind === "online"
      ? t("webAccess.reach.statusOnline")
      : kind === "cut"
        ? t("webAccess.reach.statusCut")
        : kind === "offline"
          ? t("webAccess.reach.statusOffline")
          : t("webAccess.reach.statusLoggedOut");

  const copyRow = async (id: MembershipCopyRowId, value: string) => {
    const ok = await copyTextToClipboard(value);
    if (!ok) {
      onCopyFailed();
      return;
    }
    setCopiedId(id);
    window.setTimeout(() => setCopiedId((current) => (current === id ? null : current)), 1500);
    onCopied();
  };

  return (
    <div className="rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-4">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
            {t("webAccess.reach.title")}
          </div>
          <div className="mt-1 text-sm font-medium text-[var(--color-text-primary)]">
            {statusLabel}
          </div>
          <p className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
            {t("webAccess.reach.description")}
          </p>
        </div>
        <div className="flex shrink-0 flex-wrap gap-2">
          <button
            type="button"
            onClick={onReachOn}
            disabled={!canStart || reachBusy}
            className={primaryButtonClass(reachBusy)}
          >
            {t("webAccess.reach.start")}
          </button>
          <button
            type="button"
            onClick={onReachOff}
            disabled={!canStop || reachBusy}
            className={secondaryButtonClass()}
          >
            {t("webAccess.reach.stop")}
          </button>
        </div>
      </div>

      <p className="mt-3 text-xs leading-6 text-[var(--color-text-secondary)]">
        {kind === "logged_out"
          ? t("webAccess.reach.loggedOut")
          : kind === "cut"
            ? t("webAccess.reach.cut")
            : kind === "offline"
              ? t("webAccess.reach.loggedInOffline")
              : t("webAccess.reach.online")}
      </p>

      {kind !== "logged_out" ? (
        <>
          <div className="mt-4 space-y-3">
            {rows.map((row) => (
              <div
                key={row.id}
                className="rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-3 py-3"
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="text-xs font-medium text-[var(--color-text-primary)]">
                      {t(`webAccess.reach.${row.id}Label`)}
                    </div>
                    <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
                      {row.available
                        ? t(`webAccess.reach.${row.id}Help`)
                        : t(`webAccess.reach.${row.id}Missing`)}
                    </div>
                  </div>
                  <button
                    type="button"
                    disabled={!row.available}
                    onClick={() => void copyRow(row.id, row.value)}
                    className={`${secondaryButtonClass("sm")} shrink-0`}
                  >
                    {copiedId === row.id ? t("webAccess.reach.copied") : t("webAccess.reach.copy")}
                  </button>
                </div>
                {row.available ? (
                  <pre className="mt-2 max-h-24 overflow-auto break-all whitespace-pre-wrap font-mono text-[11px] leading-5 text-[var(--color-text-secondary)]">
                    {row.value}
                  </pre>
                ) : null}
              </div>
            ))}
            {!hostnameSafe ? (
              <p className="text-xs leading-6 text-amber-700 dark:text-amber-300">
                {t("webAccess.reach.hostnameUnsafe")}
              </p>
            ) : null}
          </div>
          <div className="mt-4 space-y-3 text-xs leading-6 text-[var(--color-text-secondary)]">
            <div>
              <div className="font-medium text-[var(--color-text-primary)]">
                {t("webAccess.reach.chatgptTitle")}
              </div>
              <p className="mt-1">{t("webAccess.reach.chatgptSteps")}</p>
              <p className="mt-1">{t("webAccess.reach.chatgptAfterCut")}</p>
            </div>
            <div>
              <div className="font-medium text-[var(--color-text-primary)]">
                {t("webAccess.reach.bridgeTitle")}
              </div>
              <p className="mt-1">{t("webAccess.reach.bridgeHelp")}</p>
            </div>
            <p className="text-[var(--color-text-muted)]">{t("webAccess.reach.logoutWarning")}</p>
          </div>
        </>
      ) : null}
    </div>
  );
}
