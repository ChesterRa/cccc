// Check for a newer CCCC and update to it. Nothing asks release discovery until
// the administrator clicks Check. A host that can replace itself carries the
// update out after one confirmation; others name the command for their
// installation. A failed check is shown as failed, never as up to date.
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import * as api from "../../../services/api";
import type { LastUpdate, UpdateCheck, UpdateStatus } from "../../../services/api";
import { copyTextToClipboard } from "../../../utils/copy";
import { SettingsCard, SettingsNotice, SettingsPage, SettingsRow } from "./SettingsKit";
import { primaryButtonClass, secondaryButtonClass } from "./types";

// While CCCC replaces itself this page cannot reach it; it asks again until
// CCCC answers with this update's result.
const RETURN_POLL_MS = 2000;
const RETURN_PATIENCE_MS = 5 * 60 * 1000;

// Failures of the request itself. CCCC never answered them, so the update may
// or may not have started; only CCCC's own error says it did not.
const NO_ANSWER = new Set(["NETWORK_ERROR", "EMPTY_RESPONSE", "HTTP_ERROR", "PARSE_ERROR"]);

// What ends the wait: this update's own transaction or, when the answer to
// the start was lost, a result for this version recorded after the one CCCC
// had before the start. `undefined` means that earlier one is unknown too.
type Awaiting = { transaction: string } | { before: string | null | undefined };

type Phase =
  | { kind: "idle" }
  | { kind: "confirming"; version: string }
  | { kind: "preparing"; version: string }
  | { kind: "rejected"; reason: string }
  | {
      kind: "updating";
      version: string;
      awaiting: Awaiting;
      since: number;
      /** The answer to the start was lost, so whether it started is unknown. */
      lost: boolean;
      /** Past the patience: whether CCCC still cannot be reached, or answers without a result. */
      late: null | "notBack" | "noResult";
    };

export function UpdateTab({ isActive = true }: { isActive?: boolean }) {
  const { t, i18n } = useTranslation("settings");
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [inPlace, setInPlace] = useState(false);
  const [unavailable, setUnavailable] = useState("");
  const [check, setCheck] = useState<UpdateCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkError, setCheckError] = useState("");
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [copied, setCopied] = useState(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const loadStatus = useCallback(async () => {
    const response = await api.fetchUpdateStatus();
    if (!mounted.current) return null;
    if (!response.ok) {
      if (response.error.code === "update_unavailable") setUnavailable(response.error.message);
      return null;
    }
    setStatus(response.result.status);
    setInPlace(response.result.in_place);
    return response.result.status;
  }, []);

  useEffect(() => {
    if (isActive) void loadStatus();
  }, [isActive, loadStatus]);

  const updating = phase.kind === "updating" ? phase : null;
  useEffect(() => {
    if (!updating) return;
    let stopped = false;
    const timer = window.setInterval(async () => {
      const response = await api.fetchUpdateStatus();
      if (stopped || !mounted.current) return;
      if (response.ok) {
        // The installed version and the last result shown stay CCCC's current ones.
        setStatus(response.result.status);
        setInPlace(response.result.in_place);
        if (isResult(response.result.status.last, updating)) {
          stopped = true;
          window.clearInterval(timer);
          setCheck(null);
          setPhase({ kind: "idle" });
          return;
        }
      }
      // It keeps asking, so a late result still ends the wait.
      if (Date.now() - updating.since > RETURN_PATIENCE_MS) {
        const late = response.ok ? "noResult" : "notBack";
        if (late !== updating.late) setPhase({ ...updating, late });
      }
    }, RETURN_POLL_MS);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [updating]);

  const runCheck = async () => {
    setChecking(true);
    setCheckError("");
    setCopied(false);
    setPhase({ kind: "idle" });
    const response = await api.checkForUpdate();
    if (!mounted.current) return;
    setChecking(false);
    if (!response.ok) {
      setCheck(null);
      if (response.error.code === "update_unavailable") setUnavailable(response.error.message);
      else setCheckError(response.error.message);
      return;
    }
    setCheck(response.result.check);
    setInPlace(response.result.in_place);
  };

  const runUpdate = async (version: string) => {
    setPhase({ kind: "preparing", version });
    // CCCC's last result before this update, so that a lost answer can still be
    // told apart from an earlier update's result.
    const before = (await loadStatus()) ?? status;
    if (!mounted.current) return;
    const response = await api.startUpdate(version);
    if (!mounted.current) return;
    const waiting = { kind: "updating" as const, version, since: Date.now(), late: null };
    const transaction = response.ok ? response.result?.transaction : undefined;
    if (response.ok && response.result?.accepted === true && transaction) {
      setPhase({ ...waiting, awaiting: { transaction }, lost: false });
    } else if (!response.ok && !NO_ANSWER.has(response.error.code)) {
      setPhase({ kind: "rejected", reason: response.error.message });
    } else {
      const earlier = before ? (before.last?.transaction ?? null) : undefined;
      setPhase({ ...waiting, awaiting: { before: earlier }, lost: true });
    }
  };

  const busy =
    checking || phase.kind === "preparing" || (phase.kind === "updating" && phase.late === null);
  const actionable = check?.status === "newer" || check?.status === "channel_switch";

  return (
    <SettingsPage title={t("update.title")} description={t("update.description")}>
      {unavailable ? (
        <SettingsNotice tone="warning">{t("update.unavailable")}</SettingsNotice>
      ) : null}
      <SettingsCard
        title={t("update.versionTitle")}
        actions={
          unavailable ? undefined : (
            <button
              type="button"
              className={secondaryButtonClass("sm")}
              onClick={() => void runCheck()}
              disabled={busy}
            >
              {checking ? t("update.checking") : t("update.check")}
            </button>
          )
        }
      >
        <SettingsRow label={t("update.installed")}>
          <span className="font-mono text-[13px] text-[var(--color-text-secondary)]">
            {status?.current || "—"}
          </span>
        </SettingsRow>
        {check?.latest ? (
          <SettingsRow label={t("update.latest")}>
            <span className="inline-flex flex-wrap items-center gap-x-3 gap-y-1">
              <span className="font-mono text-[13px] text-[var(--color-text-primary)]">
                {check.latest}
              </span>
              {actionable && check.release_notes_url ? (
                <a
                  href={check.release_notes_url}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center text-[13px] text-[var(--color-accent-primary)] underline-offset-2 hover:underline"
                >
                  {t("update.releaseNotes")}
                </a>
              ) : null}
            </span>
          </SettingsRow>
        ) : null}
        {checkError || check ? (
          <div className="px-5 py-3.5">
            {checkError ? (
              <SettingsNotice tone="error">
                {t("update.checkFailed", { reason: checkError })}
              </SettingsNotice>
            ) : check && !check.latest ? (
              <SettingsNotice tone="error">
                {t("update.checkFailed", { reason: check.failure || "—" })}
              </SettingsNotice>
            ) : check && !actionable ? (
              <SettingsNotice>{settledText(check, t)}</SettingsNotice>
            ) : check?.latest ? (
              <UpdateAction
                check={check}
                latest={check.latest}
                inPlace={inPlace}
                phase={phase}
                copied={copied}
                onCopy={async (command) => setCopied(await copyTextToClipboard(command))}
                onConfirm={(version) => setPhase({ kind: "confirming", version })}
                onCancel={() => setPhase({ kind: "idle" })}
                onUpdate={(version) => void runUpdate(version)}
              />
            ) : null}
          </div>
        ) : phase.kind === "updating" ? (
          <div className="px-5 py-3.5">
            <WaitingNotice phase={phase} />
          </div>
        ) : null}
      </SettingsCard>
      {status?.last ? (
        <SettingsCard title={t("update.lastTitle")}>
          <SettingsRow label={formatTime(status.last.finished_at, i18n.language)}>
            <span
              className={
                status.last.result === "installed"
                  ? "text-[13px] text-[var(--color-text-secondary)]"
                  : "text-[13px] text-amber-700 dark:text-amber-300"
              }
            >
              {lastText(status.last, t)}
            </span>
          </SettingsRow>
        </SettingsCard>
      ) : null}
    </SettingsPage>
  );
}

function UpdateAction({
  check,
  latest,
  inPlace,
  phase,
  copied,
  onCopy,
  onConfirm,
  onCancel,
  onUpdate,
}: {
  check: UpdateCheck;
  latest: string;
  inPlace: boolean;
  phase: Phase;
  copied: boolean;
  onCopy: (command: string) => void;
  onConfirm: (version: string) => void;
  onCancel: () => void;
  onUpdate: (version: string) => void;
}) {
  const { t } = useTranslation("settings");
  if (!inPlace) {
    if (!check.command) return <SettingsNotice>{t("update.unmanaged")}</SettingsNotice>;
    const command = check.command;
    return (
      <div className="flex min-w-0 flex-col gap-2">
        <p className="text-[13px] leading-5 text-[var(--color-text-secondary)]">
          {check.installation === "pip" ? t("update.byPip") : t("update.byHand")}
        </p>
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <code className="min-w-0 flex-1 basis-64 rounded-lg bg-[var(--glass-tab-bg)] px-3 py-2 font-mono text-xs break-all text-[var(--color-text-primary)]">
            {command}
          </code>
          <button
            type="button"
            className={secondaryButtonClass("sm")}
            onClick={() => onCopy(command)}
          >
            {copied ? t("common:copied") : t("common:copy")}
          </button>
        </div>
      </div>
    );
  }
  switch (phase.kind) {
    case "confirming":
      return (
        <SettingsNotice
          tone="warning"
          action={
            <>
              <button type="button" className={secondaryButtonClass("sm")} onClick={onCancel}>
                {t("common:cancel")}
              </button>
              <button
                type="button"
                className={primaryButtonClass()}
                onClick={() => onUpdate(phase.version)}
              >
                {t("update.updateNow")}
              </button>
            </>
          }
        >
          {t("update.confirmText", { version: phase.version })}
        </SettingsNotice>
      );
    case "preparing":
      return (
        <SettingsNotice>
          <span role="status">{t("update.preparing", { version: phase.version })}</span>
        </SettingsNotice>
      );
    case "updating":
      return <WaitingNotice phase={phase} />;
    case "rejected":
      return (
        <SettingsNotice tone="error">
          {t("update.rejected", { reason: phase.reason })}
        </SettingsNotice>
      );
    default:
      return (
        <div className="flex flex-wrap items-center gap-3">
          <button type="button" className={primaryButtonClass()} onClick={() => onConfirm(latest)}>
            {t("update.updateAndRestart")}
          </button>
        </div>
      );
  }
}

/** While CCCC replaces itself, and after the patience runs out. */
function WaitingNotice({ phase }: { phase: Extract<Phase, { kind: "updating" }> }) {
  const { t } = useTranslation("settings");
  if (phase.late) {
    return (
      <SettingsNotice tone="warning">
        {t(`update.${phase.late}`, { version: phase.version })}
      </SettingsNotice>
    );
  }
  return (
    <SettingsNotice>
      <span role="status">
        {t(phase.lost ? "update.lost" : "update.updating", { version: phase.version })}
      </span>
    </SettingsNotice>
  );
}

/** Whether `last` is the result the page waits for. */
function isResult(last: LastUpdate | null, phase: Extract<Phase, { kind: "updating" }>): boolean {
  if (!last) return false;
  const { awaiting } = phase;
  if ("transaction" in awaiting) return last.transaction === awaiting.transaction;
  // Without the earlier result, an older one cannot be told apart.
  if (awaiting.before === undefined) return false;
  return last.transaction !== awaiting.before && last.to_version === phase.version;
}

type Translate = ReturnType<typeof useTranslation>["t"];

function settledText(check: UpdateCheck, t: Translate): string {
  switch (check.status) {
    case "current":
      return t("update.upToDate");
    case "installed_newer":
      return t("update.installedNewer");
    case "no_package":
      return t("update.noPackage");
    default:
      return t("update.nothingToUpdate");
  }
}

function lastText(last: LastUpdate, t: Translate): string {
  const values = {
    from: last.from_version || "—",
    to: last.to_version,
    reason: [last.stage, last.reason].filter(Boolean).join(": ") || "—",
  };
  switch (last.result) {
    case "installed":
      return t("update.last.installed", values);
    case "abandoned":
      return t("update.last.abandoned", values);
    case "not_started":
      return t("update.last.notStarted", values);
    case "unconfirmed":
      return t("update.last.unconfirmed", values);
    case "undone":
      return t("update.last.undone", values);
    case "kept":
      return t("update.last.kept", values);
    default:
      return t("update.last.other", { ...values, result: last.result });
  }
}

function formatTime(value: string, language: string): string {
  const time = new Date(value);
  if (Number.isNaN(time.getTime())) return value;
  return new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(
    time,
  );
}
