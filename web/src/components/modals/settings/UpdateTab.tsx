// Check a release or the program at CCCC's launch path, then hand the chosen
// update or restart to the host.
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import * as api from "../../../services/api";
import type {
  KeepRefusal,
  LastUpdate,
  LocalBuild,
  UpdateAgents,
  UpdateCheck,
  UpdateMode,
  UpdateStatus,
  UpdateTarget,
} from "../../../services/api";
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

// One update at a time: a published release, or the program at the
// path this CCCC was started from.
type Phase =
  | { kind: "idle" }
  | { kind: "confirming"; target: UpdateTarget }
  | { kind: "preparing"; target: UpdateTarget }
  | { kind: "rejected"; target: UpdateTarget; reason: string }
  | {
      kind: "updating";
      target: UpdateTarget;
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
  const [localRestart, setLocalRestart] = useState(false);
  const [releaseKeep, setReleaseKeep] = useState(false);
  const [unavailable, setUnavailable] = useState("");
  const [check, setCheck] = useState<UpdateCheck | null>(null);
  const [local, setLocal] = useState<LocalBuild | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkingLocal, setCheckingLocal] = useState(false);
  const [checkError, setCheckError] = useState("");
  const [localError, setLocalError] = useState("");
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
    setLocalRestart(response.result.local_restart === true);
    return response.result.status;
  }, []);

  useEffect(() => {
    if (isActive) void loadStatus();
  }, [isActive, loadStatus]);

  // A result is recorded before its agents are found; ask again until they are.
  const observing =
    isActive && phase.kind !== "updating" && status?.last?.agents?.state === "observing";
  useEffect(() => {
    if (!observing) return;
    const timer = window.setInterval(() => void loadStatus(), RETURN_POLL_MS);
    return () => window.clearInterval(timer);
  }, [observing, loadStatus]);

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
        setLocalRestart(response.result.local_restart === true);
        if (isResult(response.result.status.last, updating)) {
          stopped = true;
          window.clearInterval(timer);
          setCheck(null);
          setLocal(null);
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
    setReleaseKeep(response.result.release_keep === true);
  };

  const checkRestart = async () => {
    setCheckingLocal(true);
    setPhase({ kind: "idle" });
    setLocalError("");
    setLocal(null);
    const response = await api.checkForUpdate("local");
    if (!mounted.current) return;
    setCheckingLocal(false);
    if (!response.ok) {
      setLocalError(response.error.message);
      return;
    }
    const candidate = response.result.check.local;
    setLocal(candidate);
    setLocalRestart(response.result.local_restart);
    if (candidate?.state === "current" || candidate?.state === "rebuilt") {
      const { version, token, source_id } = candidate;
      const target = { source: "local" as const, version, token, source_id };
      setPhase({
        kind: "confirming",
        target:
          candidate.state === "current"
            ? { ...target, transaction: candidate.transaction }
            : target,
      });
    }
  };

  const runUpdate = async (target: UpdateTarget, mode: UpdateMode) => {
    setPhase({ kind: "preparing", target });
    // CCCC's last result before this update, so that a lost answer can still be
    // told apart from an earlier update's result.
    const before = (await loadStatus()) ?? status;
    if (!mounted.current) return;
    const response = await api.startUpdate(target, mode);
    if (!mounted.current) return;
    const waiting = { kind: "updating" as const, target, since: Date.now(), late: null };
    const transaction = response.ok ? response.result?.transaction : undefined;
    if (response.ok && response.result?.accepted === true && transaction) {
      setPhase({ ...waiting, awaiting: { transaction }, lost: false });
    } else if (!response.ok && !NO_ANSWER.has(response.error.code)) {
      setPhase({ kind: "rejected", target, reason: response.error.message });
    } else if (
      target.source === "local" &&
      target.transaction &&
      before?.last?.transaction !== target.transaction
    ) {
      setPhase({ ...waiting, awaiting: { transaction: target.transaction }, lost: true });
    } else {
      const earlier = before ? (before.last?.transaction ?? null) : undefined;
      setPhase({ ...waiting, awaiting: { before: earlier }, lost: true });
    }
  };

  const busy =
    checking ||
    checkingLocal ||
    phase.kind === "confirming" ||
    phase.kind === "preparing" ||
    (phase.kind === "updating" && phase.late === null);
  const actionable = check?.status === "newer" || check?.status === "channel_switch";
  // Each target's own steps show where it is offered; the other one waits.
  const releasePhase: Phase =
    phase.kind !== "idle" && phase.target.source === "release" ? phase : { kind: "idle" };
  const localPhase: Phase =
    phase.kind !== "idle" && phase.target.source === "local" ? phase : { kind: "idle" };
  const candidate = local?.state === "current" || local?.state === "rebuilt" ? local : null;

  return (
    <SettingsPage title={t("update.title")} description={t("update.description")}>
      {unavailable ? (
        <SettingsNotice tone="warning">{t("update.unavailable")}</SettingsNotice>
      ) : null}
      <SettingsCard
        title={t("update.versionTitle")}
        actions={
          unavailable ? undefined : (
            <div className="flex flex-wrap items-center gap-2">
              {localRestart ? (
                <button
                  type="button"
                  className={secondaryButtonClass("sm")}
                  onClick={() => void checkRestart()}
                  disabled={busy}
                >
                  {checkingLocal ? t("update.checking") : t("update.restart")}
                </button>
              ) : null}
              <button
                type="button"
                className={secondaryButtonClass("sm")}
                onClick={() => void runCheck()}
                disabled={busy}
              >
                {checking ? t("update.checking") : t("update.check")}
              </button>
            </div>
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
        {local && local.state !== "current" ? (
          <LocalBuildRow local={local} language={i18n.language} />
        ) : null}
        {localError ? (
          <div className="px-5 py-3.5">
            <SettingsNotice tone="error">
              {t("update.checkFailed", { reason: localError })}
            </SettingsNotice>
          </div>
        ) : null}
        {candidate && localPhase.kind !== "idle" ? (
          <div className="px-5 py-3.5">
            <LocalAction
              local={candidate}
              language={i18n.language}
              phase={localPhase}
              onCancel={() => setPhase({ kind: "idle" })}
              onStart={(target, mode) => void runUpdate(target, mode)}
            />
          </div>
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
                releaseKeep={releaseKeep}
                phase={releasePhase}
                waiting={
                  checking ||
                  checkingLocal ||
                  (phase.kind !== "idle" && releasePhase.kind === "idle")
                }
                copied={copied}
                onCopy={async (command) => setCopied(await copyTextToClipboard(command))}
                onConfirm={(version) =>
                  setPhase({ kind: "confirming", target: { source: "release", version } })
                }
                onCancel={() => setPhase({ kind: "idle" })}
                onUpdate={(target, mode) => void runUpdate(target, mode)}
              />
            ) : null}
          </div>
        ) : releasePhase.kind === "updating" ? (
          <div className="px-5 py-3.5">
            <WaitingNotice phase={releasePhase} />
          </div>
        ) : null}
      </SettingsCard>
      {status?.last ? (
        <SettingsCard title={t("update.lastTitle")}>
          <SettingsRow label={formatTime(status.last.finished_at, i18n.language)}>
            <span className="flex min-w-0 flex-col gap-0.5">
              <span
                className={
                  status.last.result === "installed" || status.last.result === "restarted"
                    ? "text-[13px] text-[var(--color-text-secondary)]"
                    : "text-[13px] text-amber-700 dark:text-amber-300"
                }
              >
                {lastText(status.last, t)}
              </span>
              <LastAgents last={status.last} />
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
  releaseKeep,
  phase,
  waiting,
  copied,
  onCopy,
  onConfirm,
  onCancel,
  onUpdate,
}: {
  check: UpdateCheck;
  latest: string;
  inPlace: boolean;
  /** Keeping agents is offered; whether this release can keep them is checked once downloaded. */
  releaseKeep: boolean;
  phase: Phase;
  /** The other target's update is under way. */
  waiting: boolean;
  copied: boolean;
  onCopy: (command: string) => void;
  onConfirm: (version: string) => void;
  onCancel: () => void;
  onUpdate: (target: UpdateTarget, mode: UpdateMode) => void;
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
              {releaseKeep ? (
                <>
                  <button
                    type="button"
                    className={primaryButtonClass()}
                    onClick={() => onUpdate(phase.target, "keep")}
                  >
                    {t("update.keepAgents")}
                  </button>
                  <button
                    type="button"
                    className={secondaryButtonClass("sm")}
                    onClick={() => onUpdate(phase.target, "restart")}
                  >
                    {t("update.restartAgents")}
                  </button>
                </>
              ) : (
                <button
                  type="button"
                  className={primaryButtonClass()}
                  onClick={() => onUpdate(phase.target, "restart")}
                >
                  {t("update.updateNow")}
                </button>
              )}
            </>
          }
        >
          {t(releaseKeep ? "update.confirmKeep" : "update.confirmText", {
            version: phase.target.version,
          })}
        </SettingsNotice>
      );
    case "preparing":
      return (
        <SettingsNotice>
          <span role="status">{t("update.preparing", { version: phase.target.version })}</span>
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
          <button
            type="button"
            className={primaryButtonClass()}
            onClick={() => onConfirm(latest)}
            disabled={waiting}
          >
            {t(releaseKeep ? "update.updateNow" : "update.updateAndRestart")}
          </button>
        </div>
      );
  }
}

/** Restart the selected program, keeping agents running when allowed. */
function LocalAction({
  local,
  language,
  phase,
  onCancel,
  onStart,
}: {
  local: Extract<LocalBuild, { state: "current" | "rebuilt" }>;
  language: string;
  phase: Phase;
  onCancel: () => void;
  onStart: (target: UpdateTarget, mode: UpdateMode) => void;
}) {
  const { t } = useTranslation("settings");
  switch (phase.kind) {
    case "confirming": {
      const { allowed, refusals } = local.keep;
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
                className={allowed ? primaryButtonClass() : secondaryButtonClass("sm")}
                onClick={() => onStart(phase.target, "keep")}
                disabled={!allowed}
              >
                {t("update.keepAgents")}
              </button>
              <button
                type="button"
                className={allowed ? secondaryButtonClass("sm") : primaryButtonClass()}
                onClick={() => onStart(phase.target, "restart")}
              >
                {t("update.restartAgents")}
              </button>
            </>
          }
        >
          <span className="flex min-w-0 flex-col gap-1.5">
            <span>
              {local.state === "rebuilt"
                ? t("update.confirmLocal", {
                    version: local.version,
                    time: formatTime(local.built_at, language),
                  })
                : t("update.confirmCurrent", { version: local.version })}
            </span>
            {allowed ? null : <KeepRefusals refusals={refusals} />}
          </span>
        </SettingsNotice>
      );
    }
    case "preparing":
      return (
        <SettingsNotice>
          <span role="status">{t("update.preparingLocal")}</span>
        </SettingsNotice>
      );
    case "updating":
      return <WaitingNotice phase={phase} />;
    case "rejected":
      return (
        <SettingsNotice tone="error">
          {t("update.rejectedLocal", { reason: phase.reason })}
        </SettingsNotice>
      );
    case "idle":
      return null;
  }
}

/** Why agents cannot keep running this time. A code this page knows reads in the page's
 *  language; any other shows CCCC's own sentence. */
function KeepRefusals({ refusals }: { refusals: KeepRefusal[] }) {
  const { t } = useTranslation("settings");
  return (
    <span className="flex min-w-0 flex-col gap-0.5">
      <span>{t("update.keepUnavailable")}</span>
      {refusals.map((refusal, index) => (
        <span key={`${refusal.code}-${refusal.actor_id ?? ""}-${index}`} className="break-words">
          {keepReason(refusal, t)}
        </span>
      ))}
    </span>
  );
}

/** The refusals this page words in its own language. */
const KEEP_REASONS = new Set([
  "keep_not_supported",
  "owner_unreachable",
  "owner_contract_mismatch",
  "mcp_registration_mismatch",
  "member_state_unconfirmed",
  "mcp_source_unconfirmed",
  "mcp_not_observed",
  "mcp_identity_unconfirmed",
  "mcp_contract_mismatch",
  "preflight_timeout",
  "runtime_not_covered",
  "remote_resource_unconfirmed",
]);

/** One refusal, led by whom it is about when it names someone. */
function keepReason(refusal: KeepRefusal, t: Translate): string {
  // Any other refusal is CCCC's own sentence, which names whom it is about.
  if (!KEEP_REASONS.has(refusal.code)) return refusal.message;
  const reason = t(`update.keepReason.${refusal.code}`);
  const name = refusal.display_name || refusal.actor_id;
  return name ? t("update.keepReasonFor", { name, reason }) : reason;
}

/** The program this CCCC was started from has changed since, whatever the release check said. */
function LocalBuildRow({
  local,
  language,
}: {
  local: Exclude<LocalBuild, { state: "current" }>;
  language: string;
}) {
  const { t } = useTranslation("settings");
  return (
    <SettingsRow label={t("update.localBuild")}>
      {local.state === "rebuilt" ? (
        <span className="inline-flex flex-wrap items-center gap-x-3 gap-y-1">
          <span className="font-mono text-[13px] text-[var(--color-text-primary)]">
            {local.version}
          </span>
          <span className="text-[13px] text-[var(--color-text-secondary)]">
            {t("update.builtAt", { time: formatTime(local.built_at, language) })}
          </span>
        </span>
      ) : (
        <span className="text-[13px] break-words text-amber-700 dark:text-amber-300">
          {local.state === "incompatible"
            ? t("update.localIncompatible", { version: local.version })
            : t("update.localUnreadable", { reason: local.reason })}
        </span>
      )}
    </SettingsRow>
  );
}

/** While CCCC replaces itself, and after the patience runs out. */
function WaitingNotice({ phase }: { phase: Extract<Phase, { kind: "updating" }> }) {
  const { t } = useTranslation("settings");
  const local = phase.target.source === "local";
  const version = phase.target.version;
  if (phase.late) {
    const late = phase.late === "noResult" && local ? "noResultLocal" : phase.late;
    return <SettingsNotice tone="warning">{t(`update.${late}`, { version })}</SettingsNotice>;
  }
  const waiting = local
    ? phase.lost
      ? "update.lostLocal"
      : "update.restartingLocal"
    : phase.lost
      ? "update.lost"
      : "update.updating";
  return (
    <SettingsNotice>
      <span role="status">{t(waiting, { version })}</span>
    </SettingsNotice>
  );
}

/** Whether `last` is the result the page waits for. */
function isResult(last: LastUpdate | null, phase: Extract<Phase, { kind: "updating" }>): boolean {
  if (!last) return false;
  const { awaiting } = phase;
  if ("transaction" in awaiting) return last.transaction === awaiting.transaction;
  // Without the earlier result, an older one cannot be told apart.
  if (awaiting.before === undefined || last.transaction === awaiting.before) return false;
  // This fallback identifies a rebuild by file and source. An offer to
  // restart the current program already names its own transaction.
  if (phase.target.source === "local") {
    return (
      last.source === "local" &&
      last.token === phase.target.token &&
      last.source_id === phase.target.source_id
    );
  }
  return last.to_version === phase.target.version;
}

type Translate = ReturnType<typeof useTranslation>["t"];

/** The agents the update stopped or kept, as CCCC found them once it was back. */
function LastAgents({ last }: { last: LastUpdate }) {
  const { t } = useTranslation("settings");
  const agents: UpdateAgents = last.agents ?? {
    state: "not_observed",
    reattached: [],
    restarted: [],
    not_back: [],
    unconfirmed: [],
  };
  // Agents meant to keep running were not looked for after the update stopped
  // CCCC: say so rather than imply they were kept.
  if (agents.state === "not_observed") {
    return last.mode === "keep" && last.result !== "not_started" ? (
      <span className="text-[13px] text-amber-700 dark:text-amber-300">
        {t("update.agents.notObserved")}
      </span>
    ) : null;
  }
  if (agents.state === "observing") {
    return (
      <span role="status" className="text-[13px] text-[var(--color-text-secondary)]">
        {t("update.agents.observing")}
      </span>
    );
  }
  const rows = (
    [
      ["reattached", agents.reattached, false],
      ["restarted", agents.restarted, false],
      ["notBack", agents.not_back, true],
      ["unconfirmed", agents.unconfirmed, true],
    ] as const
  ).filter(([, members]) => members.length > 0);
  return rows.map(([key, members, unsettled]) => (
    <span
      key={key}
      className={
        unsettled
          ? "text-[13px] break-words text-amber-700 dark:text-amber-300"
          : "text-[13px] break-words text-[var(--color-text-secondary)]"
      }
    >
      {t(`update.agents.${key}`, {
        names: members
          .map((member) => member.title || member.actor_id)
          .join(t("update.agents.separator")),
      })}
    </span>
  ));
}

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
  // A restart into a rebuilt program replaces no files and has no earlier
  // program to start again.
  const local = last.source === "local";
  switch (last.result) {
    case "installed":
      return t("update.last.installed", values);
    case "abandoned":
      return t(local ? "update.last.abandonedLocal" : "update.last.abandoned", values);
    case "not_started":
      return t(local ? "update.last.notStartedLocal" : "update.last.notStarted", values);
    case "unconfirmed":
      return t(local ? "update.last.unconfirmedLocal" : "update.last.unconfirmed", values);
    case "undone":
      return t("update.last.undone", values);
    case "kept":
      return t("update.last.kept", values);
    case "restarted":
      return t("update.last.restarted", values);
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
