import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useModalA11y } from "../../hooks/useModalA11y";
import type { ApiResponse } from "../../services/api/base";
import {
  acceptProcessRecovery,
  fetchProcessRecovery,
  retryProcessRecovery,
  type ProcessRecoveryDiagnostic,
  type ProcessRecoveryOwner,
  type ProcessRecoveryRecord,
  type ProcessRecoveryRetryResult,
  type ProcessRecoverySnapshot,
} from "../../services/api/processRecovery";
import { formatMessageTimestamp } from "../../utils/time";
import { BodyPortal } from "../ui/BodyPortal";
import { Button } from "../ui/button";
import { ModalFrame } from "./ModalFrame";

// A recovery request can wait on cleanup; past this the result is treated as unconfirmed.
const REQUEST_TIMEOUT_MS = 30_000;
const KINDS = new Set(["process_group", "launch_intent", "opaque_evidence"]);
const ORIGINS = new Set(["previous_boot", "other_namespace"]);
const REASONS = new Set([
  "launch_incomplete",
  "legacy_identity",
  "identity_unconfirmed",
  "foreign_namespace",
  "origin_unknown",
  "cleanup_unconfirmed",
  "evidence_invalid",
  "members_unconfirmed",
  "signal_failed",
  "evidence_unreadable",
]);
const HINTS = new Set([
  "inspect_permissions",
  "inspect_storage",
  "use_offline_review",
  "retry_later",
  "contact_support",
]);

type OwnerView =
  | { state: "loading" }
  | { state: "ready"; snapshot: ProcessRecoverySnapshot }
  | { state: "unavailable" }
  | { state: "denied" };
type Views = Partial<Record<ProcessRecoveryOwner, OwnerView>>;
type Entry = { owner: ProcessRecoveryOwner; record: ProcessRecoveryRecord };
type BulkStop =
  | "changed"
  | "new_records"
  | "diagnostics"
  | "read_failed"
  | "denied"
  | "uncertain"
  | "user";
type Bulk =
  | { phase: "confirm"; items: Entry[] }
  | { phase: "running"; items: Entry[]; done: number }
  | { phase: "finished"; accepted: Entry[]; stop: BulkStop | null };

const entryKey = (owner: ProcessRecoveryOwner, id: string) => `${owner}:${id}`;

async function bounded<T>(outer: AbortSignal, run: (signal: AbortSignal) => Promise<T>) {
  const controller = new AbortController();
  const abort = () => controller.abort();
  outer.addEventListener("abort", abort);
  const timer = window.setTimeout(abort, REQUEST_TIMEOUT_MS);
  try {
    return await run(controller.signal);
  } finally {
    window.clearTimeout(timer);
    outer.removeEventListener("abort", abort);
  }
}

// Each field the review shows or sends back.
function isRecord(value: unknown): value is ProcessRecoveryRecord {
  const record = value as Partial<ProcessRecoveryRecord> | null | undefined;
  return (
    typeof record?.id === "string" &&
    typeof record.version === "string" &&
    typeof record.kind === "string" &&
    typeof record.origin === "string" &&
    typeof record.reason === "string" &&
    (record.created_at_unix_seconds === null ||
      typeof record.created_at_unix_seconds === "number") &&
    typeof record.retryable === "boolean" &&
    typeof record.acceptance_supported === "boolean"
  );
}

function isDiagnostic(value: unknown): value is ProcessRecoveryDiagnostic {
  const diagnostic = value as Partial<ProcessRecoveryDiagnostic> | null | undefined;
  return (
    typeof diagnostic?.reason === "string" &&
    typeof diagnostic.hint === "string" &&
    (diagnostic.relative_path === undefined || typeof diagnostic.relative_path === "string")
  );
}

const isDiagnostics = (value: unknown): value is ProcessRecoveryDiagnostic[] =>
  Array.isArray(value) && value.every(isDiagnostic);

// A malformed reply is not an empty ledger.
function isSnapshot(value: unknown, owner: ProcessRecoveryOwner): value is ProcessRecoverySnapshot {
  const snapshot = value as Partial<ProcessRecoverySnapshot> | null;
  return (
    snapshot?.owner === owner &&
    Array.isArray(snapshot.records) &&
    snapshot.records.every(isRecord) &&
    isDiagnostics(snapshot.diagnostics)
  );
}

// A successful status alone is no receipt: an empty or unexpected body leaves the outcome unknown.
function acceptReceipt(response: ApiResponse<unknown>, owner: ProcessRecoveryOwner) {
  const result = response.ok ? (response.result as { owner?: unknown; accepted?: unknown }) : null;
  return result?.owner === owner && result.accepted === true;
}

function retryReceipt(
  response: ApiResponse<ProcessRecoveryRetryResult>,
  owner: ProcessRecoveryOwner,
): ProcessRecoveryRetryResult | null {
  const result = response.ok ? response.result : null;
  return result?.owner === owner &&
    (result.result === "cleared" || result.result === "still_pending") &&
    isDiagnostics(result.diagnostics) &&
    (result.record === null || isRecord(result.record))
    ? result
    : null;
}

function applyRetry(
  view: OwnerView | undefined,
  id: string,
  result: ProcessRecoveryRetryResult,
): OwnerView | null {
  if (view?.state !== "ready") return null;
  const records = view.snapshot.records.flatMap((record) =>
    record.id !== id ? [record] : result.record ? [result.record] : [],
  );
  return {
    state: "ready",
    snapshot: { ...view.snapshot, records, diagnostics: result.diagnostics },
  };
}

function readyRecords(views: Views, owners: ProcessRecoveryOwner[]): Entry[] {
  return owners.flatMap((owner) => {
    const view = views[owner];
    return view?.state === "ready"
      ? view.snapshot.records.map((record) => ({ owner, record }))
      : [];
  });
}

function diagnosticsSignature(views: Views, owners: ProcessRecoveryOwner[]) {
  return JSON.stringify(
    owners.map((owner) => {
      const view = views[owner];
      return view?.state === "ready" ? view.snapshot.diagnostics : null;
    }),
  );
}

/** Why a bulk accept must stop after re-reading every shown owner, if it must. */
function bulkStop(
  views: Views,
  owners: ProcessRecoveryOwner[],
  remaining: Entry[],
  reviewed: Set<string>,
  diagnostics: string,
): BulkStop | null {
  if (owners.some((owner) => views[owner]?.state !== "ready")) return "read_failed";
  const current = new Map(
    readyRecords(views, owners).map((entry) => [entryKey(entry.owner, entry.record.id), entry]),
  );
  for (const item of remaining) {
    const now = current.get(entryKey(item.owner, item.record.id));
    if (!now || now.record.version !== item.record.version || !now.record.acceptance_supported)
      return "changed";
  }
  if ([...current.keys()].some((key) => !reviewed.has(key))) return "new_records";
  if (diagnosticsSignature(views, owners) !== diagnostics) return "diagnostics";
  return null;
}

type Props = {
  isOpen: boolean;
  /** Owners to review, in display order; a blocked start names only its own. */
  owners: ProcessRecoveryOwner[];
  isDark: boolean;
  onClose: () => void;
};

/** Review of process records CCCC could not confirm as ended. Reads only when opened. */
export function ProcessRecoveryDialog(props: Props) {
  if (!props.isOpen || props.owners.length === 0) return null;
  return <ProcessRecoveryReview {...props} />;
}

function ProcessRecoveryReview({ owners: requestedOwners, isDark, onClose }: Props) {
  const { t } = useTranslation("modals");
  // A request may still take effect after the page stops waiting, so the review stays open until
  // the action can show its outcome. An action is a request and the re-read after it, each with
  // its own timeout, so an open page usually waits at most two of them.
  const [busy, setBusy] = useState<string | null>(null);
  const close = () => {
    if (!busy) onClose();
  };
  const { modalRef } = useModalA11y(true, close);
  const ownerKey = requestedOwners.join(",");
  const owners = useMemo(() => ownerKey.split(",") as ProcessRecoveryOwner[], [ownerKey]);
  const [views, setViews] = useState<Views>({});
  const [notices, setNotices] = useState<Partial<Record<ProcessRecoveryOwner, string>>>({});
  const [confirming, setConfirming] = useState<string | null>(null);
  const [bulk, setBulk] = useState<Bulk | null>(null);
  const lifetime = useRef<AbortController | null>(null);
  const acceptButtons = useRef(new Map<string, HTMLButtonElement>());
  const confirmButton = useRef<HTMLButtonElement>(null);
  const bulkButton = useRef<HTMLButtonElement>(null);
  const bulkStopRequested = useRef(false);

  const read = useCallback(async (owner: ProcessRecoveryOwner, signal: AbortSignal) => {
    const response = await bounded(signal, (inner) => fetchProcessRecovery(owner, inner));
    if (response.ok && isSnapshot(response.result, owner))
      return { state: "ready", snapshot: response.result } satisfies OwnerView;
    if (!response.ok && response.error.code === "permission_denied")
      return { state: "denied" } satisfies OwnerView;
    return { state: "unavailable" } satisfies OwnerView;
  }, []);

  const reload = useCallback(
    async (targets: ProcessRecoveryOwner[], signal: AbortSignal) => {
      const results = await Promise.all(
        targets.map(async (owner) => [owner, await read(owner, signal)] as const),
      );
      if (signal.aborted) return null;
      const next: Views = Object.fromEntries(results);
      setViews((current) => ({ ...current, ...next }));
      return next;
    },
    [read],
  );

  useEffect(() => {
    const controller = new AbortController();
    lifetime.current = controller;
    setViews(Object.fromEntries(owners.map((owner) => [owner, { state: "loading" }])));
    void reload(owners, controller.signal);
    return () => controller.abort();
  }, [owners, reload]);

  useEffect(() => {
    if (confirming) confirmButton.current?.focus();
  }, [confirming]);
  useEffect(() => {
    if (bulk?.phase === "confirm") bulkButton.current?.focus();
  }, [bulk?.phase]);

  const note = (owner: ProcessRecoveryOwner, text: string) =>
    setNotices((current) => ({ ...current, [owner]: text }));

  // After a refused or lost reply the action may still have taken effect: list again, never replay.
  const settle = async (owner: ProcessRecoveryOwner, code: string, signal: AbortSignal) => {
    await reload([owner], signal);
    if (signal.aborted) return;
    note(
      owner,
      t(
        code === "process_recovery_changed"
          ? "processRecovery.changed"
          : code === "permission_denied"
            ? "processRecovery.denied"
            : code === "process_recovery_unavailable"
              ? "processRecovery.actionUnavailable"
              : "processRecovery.uncertain",
      ),
    );
  };

  const run = async (key: string, work: (signal: AbortSignal) => Promise<void>) => {
    const signal = lifetime.current?.signal;
    if (!signal || signal.aborted || busy) return;
    setBusy(key);
    try {
      await work(signal);
    } finally {
      if (!signal.aborted) setBusy(null);
    }
  };

  const retry = (owner: ProcessRecoveryOwner, record: ProcessRecoveryRecord) =>
    run(entryKey(owner, record.id), async (signal) => {
      setNotices((current) => ({ ...current, [owner]: undefined }));
      const response = await bounded(signal, (inner) => retryProcessRecovery(owner, record, inner));
      if (signal.aborted) return;
      if (!response.ok) return settle(owner, response.error.code, signal);
      const result = retryReceipt(response, owner);
      if (!result) return settle(owner, "", signal);
      const applied = applyRetry(views[owner], record.id, result);
      if (applied) setViews((current) => ({ ...current, [owner]: applied }));
      else await reload([owner], signal);
      note(
        owner,
        t(result.result === "cleared" ? "processRecovery.cleared" : "processRecovery.stillPending"),
      );
    });

  const accept = (owner: ProcessRecoveryOwner, record: ProcessRecoveryRecord) =>
    run(entryKey(owner, record.id), async (signal) => {
      setNotices((current) => ({ ...current, [owner]: undefined }));
      const response = await bounded(signal, (inner) =>
        acceptProcessRecovery(owner, record, inner),
      );
      if (signal.aborted) return;
      setConfirming(null);
      if (!response.ok) return settle(owner, response.error.code, signal);
      if (!acceptReceipt(response, owner)) return settle(owner, "", signal);
      await reload([owner], signal);
      if (!signal.aborted) note(owner, t("processRecovery.accepted"));
    });

  const cancelConfirm = () => {
    const key = confirming;
    setConfirming(null);
    if (key) requestAnimationFrame(() => acceptButtons.current.get(key)?.focus());
  };

  const acceptable = readyRecords(views, owners).filter(
    (entry) => entry.record.acceptance_supported,
  );

  const acceptAll = (items: Entry[]) =>
    run("bulk", async (signal) => {
      // Accepting is one request per record, never one transaction: stop at the first surprise.
      const reviewed = new Set(
        readyRecords(views, owners).map((entry) => entryKey(entry.owner, entry.record.id)),
      );
      const diagnostics = diagnosticsSignature(views, owners);
      const accepted: Entry[] = [];
      let stop: BulkStop | null = null;
      setNotices({});
      bulkStopRequested.current = false;
      setBulk({ phase: "running", items, done: 0 });
      for (const [index, item] of items.entries()) {
        if (bulkStopRequested.current) {
          stop = "user";
          break;
        }
        const response = await bounded(signal, (inner) =>
          acceptProcessRecovery(item.owner, item.record, inner),
        );
        if (signal.aborted) return;
        if (!acceptReceipt(response, item.owner)) {
          const code = response.ok ? "" : response.error.code;
          stop =
            code === "process_recovery_changed"
              ? "changed"
              : code === "permission_denied"
                ? "denied"
                : "uncertain";
          await reload(owners, signal);
          break;
        }
        accepted.push(item);
        setBulk({ phase: "running", items, done: index + 1 });
        const next = await reload(owners, signal);
        if (!next) return;
        // After the last record there is nothing left to protect; the list shows what changed.
        if (index + 1 === items.length) break;
        stop = bulkStop(next, owners, items.slice(index + 1), reviewed, diagnostics);
        if (stop) break;
      }
      if (!signal.aborted) setBulk({ phase: "finished", accepted, stop });
    });

  const ownerName = (owner: ProcessRecoveryOwner) => t(`processRecovery.ownerName.${owner}`);
  const kindText = (kind: string) => t(`processRecovery.kinds.${KINDS.has(kind) ? kind : "other"}`);
  const reasonText = (reason: string) =>
    t(`processRecovery.reasons.${REASONS.has(reason) ? reason : "other"}`);
  const hintText = (owner: ProcessRecoveryOwner, hint: string) =>
    hint === "retry_later" && owner === "web"
      ? t("processRecovery.hints.retry_later_voice")
      : t(`processRecovery.hints.${HINTS.has(hint) ? hint : "other"}`);
  const originText = (origin: string) =>
    origin === "same_origin"
      ? ""
      : t(`processRecovery.origins.${ORIGINS.has(origin) ? origin : "unknown"}`);
  const entryText = (entry: Entry) => `${ownerName(entry.owner)} · ${kindText(entry.record.kind)}`;

  const allEmpty = owners.every((owner) => {
    const view = views[owner];
    return (
      view?.state === "ready" &&
      view.snapshot.records.length === 0 &&
      view.snapshot.diagnostics.length === 0
    );
  });
  const locked = Boolean(busy) || bulk?.phase === "confirm" || bulk?.phase === "running";

  const recordRow = (owner: ProcessRecoveryOwner, record: ProcessRecoveryRecord) => {
    const key = entryKey(owner, record.id);
    const origin = originText(record.origin);
    // A time the browser cannot represent is left out rather than failing the whole review.
    const created =
      typeof record.created_at_unix_seconds === "number"
        ? new Date(record.created_at_unix_seconds * 1000)
        : null;
    const recorded =
      created && Number.isFinite(created.getTime())
        ? t("processRecovery.recordedAt", { time: formatMessageTimestamp(created.toISOString()) })
        : "";
    const context = [origin, recorded].filter(Boolean).join(" · ");
    return (
      <div key={key} data-process-recovery-record className="px-4 py-3">
        <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
          <div className="min-w-0">
            <div className="break-words text-sm font-medium text-[var(--color-text-primary)]">
              {kindText(record.kind)}
            </div>
            <div className="mt-0.5 break-words text-xs leading-5 text-[var(--color-text-secondary)]">
              {reasonText(record.reason)}
            </div>
            {context ? (
              <div className="mt-0.5 text-xs text-[var(--color-text-muted)]">{context}</div>
            ) : null}
          </div>
          {record.retryable || record.acceptance_supported ? (
            <div className="flex shrink-0 flex-wrap gap-2">
              {record.retryable ? (
                <Button
                  type="button"
                  size="sm"
                  variant="secondary"
                  data-lcd-key
                  className="max-sm:min-h-11"
                  disabled={locked || confirming !== null}
                  onClick={() => void retry(owner, record)}
                >
                  {busy === key && confirming !== key
                    ? t("processRecovery.checking")
                    : t("processRecovery.retry")}
                </Button>
              ) : null}
              {record.acceptance_supported ? (
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  data-lcd-key
                  className="max-sm:min-h-11"
                  aria-expanded={confirming === key}
                  ref={(element) => {
                    if (element) acceptButtons.current.set(key, element);
                    else acceptButtons.current.delete(key);
                  }}
                  disabled={locked || confirming !== null}
                  onClick={() => setConfirming(key)}
                >
                  {t("processRecovery.accept")}
                </Button>
              ) : null}
            </div>
          ) : null}
        </div>
        {confirming === key ? (
          <div
            data-process-recovery-confirm
            className="mt-3 space-y-2 rounded-lg border border-[var(--glass-border-subtle)] p-3 text-xs leading-5 text-[var(--color-text-secondary)]"
          >
            <p>{t("processRecovery.acceptRisk")}</p>
            {record.kind === "opaque_evidence" ? (
              <p>{t("processRecovery.acceptRiskOpaque")}</p>
            ) : null}
            <div className="flex flex-wrap gap-2 pt-1">
              <Button
                type="button"
                size="sm"
                variant="destructive"
                data-lcd-key
                className="max-sm:min-h-11"
                ref={confirmButton}
                disabled={Boolean(busy)}
                onClick={() => void accept(owner, record)}
              >
                {busy === key ? t("processRecovery.accepting") : t("processRecovery.confirmAccept")}
              </Button>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                data-lcd-key
                className="max-sm:min-h-11"
                disabled={Boolean(busy)}
                onClick={cancelConfirm}
              >
                {t("common:cancel")}
              </Button>
            </div>
          </div>
        ) : null}
      </div>
    );
  };

  const diagnosticsBlock = (
    owner: ProcessRecoveryOwner,
    diagnostics: ProcessRecoveryDiagnostic[],
  ) => {
    if (diagnostics.length === 0) return null;
    const offline = diagnostics.some((diagnostic) => diagnostic.hint === "use_offline_review");
    const command = `cccc daemon process-recovery --owner ${owner}`;
    return (
      <div className="mt-3 space-y-2 text-xs leading-5 text-[var(--color-text-secondary)]">
        {diagnostics.map((diagnostic, index) => (
          <div
            key={`${diagnostic.reason}:${diagnostic.relative_path ?? index}`}
            data-process-recovery-diagnostic
            className="rounded-lg border border-[var(--glass-border-subtle)] px-3 py-2"
          >
            <p className="break-words">{reasonText(diagnostic.reason)}</p>
            <p className="break-words">{hintText(owner, diagnostic.hint)}</p>
            {diagnostic.relative_path ? (
              <code className="mt-1 block select-all break-all font-mono text-[11px] text-[var(--color-text-muted)]">
                {diagnostic.relative_path}
              </code>
            ) : null}
          </div>
        ))}
        {offline ? (
          <div className="rounded-lg border border-[var(--glass-border-subtle)] px-3 py-2">
            <pre className="select-all overflow-x-auto whitespace-pre font-mono text-[11px] leading-5 text-[var(--color-text-primary)]">
              {`${command} list\n${command} retry --id <id> --version <version>\n${command} accept --id <id> --version <version> --accept-risk`}
            </pre>
            {owner === "web" ? (
              <p className="mt-1">{t("processRecovery.offlineWebRunning")}</p>
            ) : null}
          </div>
        ) : null}
      </div>
    );
  };

  const ownerSection = (owner: ProcessRecoveryOwner) => {
    const view = views[owner] ?? { state: "loading" };
    const titleId = `process-recovery-${owner}`;
    return (
      <section key={owner} aria-labelledby={titleId} data-process-recovery-owner={owner}>
        <h3
          id={titleId}
          className="text-xs font-semibold tracking-wide text-[var(--color-text-muted)]"
        >
          {t(`processRecovery.ownerSection.${owner}`)}
          {view.state === "ready" ? ` · ${view.snapshot.records.length}` : ""}
        </h3>
        {notices[owner] ? (
          <p role="status" className="mt-2 text-xs text-[var(--color-text-secondary)]">
            {notices[owner]}
          </p>
        ) : null}
        {view.state === "loading" ? (
          <p className="mt-2 text-sm text-[var(--color-text-muted)]">{t("common:loading")}</p>
        ) : view.state === "denied" ? (
          <p className="mt-2 text-sm text-[var(--color-text-secondary)]">
            {t("processRecovery.denied")}
          </p>
        ) : view.state === "unavailable" ? (
          <div className="mt-2 flex flex-wrap items-center gap-3">
            <p className="min-w-0 text-sm text-[var(--color-text-secondary)]">
              {t("processRecovery.unavailable", { owner: ownerName(owner) })}
            </p>
            <Button
              type="button"
              size="sm"
              variant="secondary"
              data-lcd-key
              className="max-sm:min-h-11"
              disabled={locked}
              onClick={() =>
                void run(`reload:${owner}`, async (signal) => {
                  setViews((current) => ({ ...current, [owner]: { state: "loading" } }));
                  await reload([owner], signal);
                })
              }
            >
              {t("processRecovery.reload")}
            </Button>
          </div>
        ) : (
          <>
            {view.snapshot.records.length > 0 ? (
              <div className="mt-2 divide-y divide-[var(--glass-border-subtle)] rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)]">
                {view.snapshot.records.map((record) => recordRow(owner, record))}
              </div>
            ) : (
              <p className="mt-2 text-sm text-[var(--color-text-muted)]">
                {t("processRecovery.noRecords")}
              </p>
            )}
            {diagnosticsBlock(owner, view.snapshot.diagnostics)}
          </>
        )}
      </section>
    );
  };

  const bulkPanel = () => {
    if (!bulk) return null;
    if (bulk.phase !== "finished") {
      const running = bulk.phase === "running";
      const opaque = bulk.items.some((item) => item.record.kind === "opaque_evidence");
      return (
        <div
          data-process-recovery-bulk={bulk.phase}
          className="space-y-2 rounded-xl border border-[var(--glass-border-subtle)] p-3 text-xs leading-5 text-[var(--color-text-secondary)]"
        >
          <p className="text-sm font-medium text-[var(--color-text-primary)]">
            {t("processRecovery.bulkTitle", { count: bulk.items.length })}
          </p>
          <ul className="list-disc space-y-0.5 pl-5">
            {bulk.items.map((item) => (
              <li key={entryKey(item.owner, item.record.id)}>{entryText(item)}</li>
            ))}
          </ul>
          <p>{t("processRecovery.acceptRisk")}</p>
          {opaque ? <p>{t("processRecovery.acceptRiskOpaque")}</p> : null}
          {running ? (
            <div className="flex flex-wrap items-center gap-3 pt-1">
              <p role="status" className="min-w-0 flex-1">
                {t("processRecovery.bulkProgress", { done: bulk.done, total: bulk.items.length })}
              </p>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                data-lcd-key
                className="max-sm:min-h-11"
                onClick={() => {
                  bulkStopRequested.current = true;
                }}
              >
                {t("processRecovery.bulkStop")}
              </Button>
            </div>
          ) : (
            <div className="flex flex-wrap gap-2 pt-1">
              <Button
                type="button"
                size="sm"
                variant="destructive"
                data-lcd-key
                className="max-sm:min-h-11"
                ref={bulkButton}
                onClick={() => void acceptAll(bulk.items)}
              >
                {t("processRecovery.bulkConfirm")}
              </Button>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                data-lcd-key
                className="max-sm:min-h-11"
                onClick={() => setBulk(null)}
              >
                {t("common:cancel")}
              </Button>
            </div>
          )}
        </div>
      );
    }
    return (
      <div
        data-process-recovery-bulk="finished"
        role="status"
        className="space-y-2 rounded-xl border border-[var(--glass-border-subtle)] p-3 text-xs leading-5 text-[var(--color-text-secondary)]"
      >
        <p className="text-sm font-medium text-[var(--color-text-primary)]">
          {bulk.stop
            ? t(`processRecovery.bulkStopped.${bulk.stop}`)
            : t("processRecovery.bulkDone", { count: bulk.accepted.length })}
        </p>
        {bulk.stop && bulk.accepted.length > 0 ? (
          <>
            <p>{t("processRecovery.bulkAcceptedBefore", { count: bulk.accepted.length })}</p>
            <ul className="list-disc space-y-0.5 pl-5">
              {bulk.accepted.map((item) => (
                <li key={entryKey(item.owner, item.record.id)}>{entryText(item)}</li>
              ))}
            </ul>
          </>
        ) : null}
        {bulk.stop ? <p>{t("processRecovery.bulkReview")}</p> : null}
        <Button
          type="button"
          size="sm"
          variant="ghost"
          data-lcd-key
          className="max-sm:min-h-11"
          onClick={() => setBulk(null)}
        >
          {t("processRecovery.bulkDismiss")}
        </Button>
      </div>
    );
  };

  return (
    <BodyPortal>
      <ModalFrame
        isDark={isDark}
        onClose={close}
        closeDisabled={Boolean(busy)}
        titleId="process-recovery-title"
        title={
          <h2 className="text-base font-semibold text-[var(--color-text-primary)]">
            {t("processRecovery.title")}
          </h2>
        }
        closeAriaLabel={t("common:close")}
        panelClassName="w-full max-w-lg max-h-[100dvh] sm:max-h-[calc(100dvh-8rem)]"
        modalRef={modalRef}
        footerActions={
          acceptable.length >= 2 && !bulk ? (
            <div className="flex justify-end">
              <Button
                type="button"
                variant="outline"
                data-lcd-key
                disabled={locked || confirming !== null}
                onClick={() => setBulk({ phase: "confirm", items: acceptable })}
              >
                {t("processRecovery.acceptAll", { count: acceptable.length })}
              </Button>
            </div>
          ) : undefined
        }
      >
        <div className="min-h-0 flex-1 space-y-4 overflow-auto p-4 sm:p-5">
          {allEmpty ? (
            <>
              {bulkPanel()}
              <p className="py-6 text-center text-sm text-[var(--color-text-muted)]">
                {t("processRecovery.empty")}
              </p>
            </>
          ) : (
            <>
              <p className="text-sm leading-6 text-[var(--color-text-secondary)]">
                {t("processRecovery.intro")}
              </p>
              {bulkPanel()}
              {owners.map(ownerSection)}
            </>
          )}
        </div>
      </ModalFrame>
    </BodyPortal>
  );
}
