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
  membership: MembershipState | null;
  membershipBusy: boolean;
  membershipError: string;
  reachBusy: boolean;
  reachAction: "starting" | "stopping" | null;
  onOpenAccount: () => void;
  onReachOn: () => void;
  onReachOff: () => void;
  onCopied: () => void;
  onCopyFailed: () => void;
}

export function ReachMembershipSection({
  membership,
  membershipBusy,
  membershipError,
  reachBusy,
  reachAction,
  onOpenAccount,
  onReachOn,
  onReachOff,
  onCopied,
  onCopyFailed,
}: ReachMembershipSectionProps) {
  const { t } = useTranslation("settings");
  const [copiedId, setCopiedId] = useState<MembershipCopyRowId | null>(null);
  const kind = membership
    ? membershipPanelKind(membership)
    : membershipBusy
      ? "loading"
      : membershipError
        ? "unavailable"
        : "loading";
  const rows = membershipCopyRows(membership);
  const hostname = String(membership?.hostname || "").trim();
  const hostnameSafe = !hostname || hostnameLooksTokenless(hostname);
  const reachSupported = membership?.reach_supported !== false;
  const canStart = kind === "offline" && reachSupported;
  const canStop = kind === "online" || Boolean(membership?.in_reach);
  const visibleError = membershipError || String(membership?.last_error || "").trim();

  const statusLabel =
    reachAction === "starting"
      ? t("webAccess.reach.statusStarting")
      : reachAction === "stopping"
        ? t("webAccess.reach.statusStopping")
        : !reachSupported && (kind === "offline" || kind === "online")
          ? t("webAccess.reach.statusUnsupported")
          : kind === "online"
            ? t("webAccess.reach.statusOnline")
            : kind === "cut"
              ? t("webAccess.reach.statusCut")
              : kind === "offline"
                ? t("webAccess.reach.statusOffline")
                : kind === "pending"
                  ? t("webAccess.reach.statusPending")
                  : kind === "unavailable"
                    ? t("webAccess.reach.statusUnavailable")
                    : kind === "loading"
                      ? t("webAccess.reach.statusLoading")
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
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
            {t("webAccess.reach.title")}
          </div>
          <div
            className="mt-1 text-sm font-medium text-[var(--color-text-primary)]"
            role="status"
            aria-live="polite"
          >
            {statusLabel}
          </div>
          <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
            {kind === "logged_out"
              ? t("webAccess.reach.accountRequired")
              : kind === "pending"
                ? t("webAccess.reach.accountPending")
                : kind === "cut"
                  ? t("webAccess.reach.accountCut")
                  : kind === "loading"
                    ? t("webAccess.reach.loading")
                    : kind === "unavailable"
                      ? t("webAccess.reach.loadFailed")
                      : !reachSupported
                        ? t("webAccess.reach.unsupported")
                        : t("webAccess.reach.description")}
          </p>
        </div>

        {kind === "logged_out" || kind === "pending" || kind === "cut" || kind === "unavailable" ? (
          <button
            type="button"
            onClick={onOpenAccount}
            disabled={membershipBusy}
            className={`${primaryButtonClass(membershipBusy)} shrink-0`}
          >
            {t("webAccess.reach.openAccountSettings")}
          </button>
        ) : (
          <div className="flex shrink-0 flex-wrap gap-2">
            <button type="button" onClick={onOpenAccount} className={secondaryButtonClass()}>
              {t("webAccess.reach.openAccountSettings")}
            </button>
            <button
              type="button"
              onClick={onReachOn}
              disabled={!canStart || reachBusy || membershipBusy}
              className={primaryButtonClass(reachBusy)}
            >
              {t("webAccess.reach.start")}
            </button>
            <button
              type="button"
              onClick={onReachOff}
              disabled={!canStop || reachBusy || membershipBusy}
              className={secondaryButtonClass()}
            >
              {t("webAccess.reach.stop")}
            </button>
          </div>
        )}
      </div>

      {visibleError ? (
        <p className="mt-3 text-xs leading-5 text-red-600 dark:text-red-300" role="alert">
          {visibleError}
        </p>
      ) : null}

      {kind === "offline" || kind === "online" ? (
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
                  <div className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
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
            <p className="text-xs leading-5 text-amber-700 dark:text-amber-300">
              {t("webAccess.reach.hostnameUnsafe")}
            </p>
          ) : null}
          <p className="text-xs leading-5 text-[var(--color-text-muted)]">
            {t("webAccess.reach.connectorManaged")}
          </p>
        </div>
      ) : null}
    </div>
  );
}
