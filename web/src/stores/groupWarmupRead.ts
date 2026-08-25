import * as api from "../services/api";
import { INITIAL_LEDGER_TAIL_LIMIT } from "./groupStoreCore";

export type GroupWarmupRead = {
  group: Awaited<ReturnType<typeof api.fetchGroup>>;
  ledgerTail: Awaited<ReturnType<typeof api.fetchLedgerTail>>;
  actors: Awaited<ReturnType<typeof api.fetchActors>>;
};

const COMPLETED_WARMUP_REUSE_MS = 5_000;

type WarmupReadEntry = { promise: Promise<GroupWarmupRead>; expiresAt: number };

const warmupReads = new Map<string, WarmupReadEntry>();

function activeWarmupRead(groupId: string): WarmupReadEntry | undefined {
  const entry = warmupReads.get(groupId);
  if (!entry || entry.expiresAt >= Date.now()) return entry;
  warmupReads.delete(groupId);
  return undefined;
}

export function getGroupWarmupRead(groupId: string): Promise<GroupWarmupRead> | undefined {
  return activeWarmupRead(String(groupId || "").trim())?.promise;
}

export function startGroupWarmupRead(groupId: string): Promise<GroupWarmupRead> {
  const gid = String(groupId || "").trim();
  const existing = activeWarmupRead(gid);
  if (existing) return existing.promise;

  const request = Promise.all([
    api.fetchGroup(gid),
    api.fetchLedgerTail(gid, INITIAL_LEDGER_TAIL_LIMIT, { includeStatuses: false }),
    api.fetchActors(gid, false, undefined, { includeInternal: true }),
  ]).then(([group, ledgerTail, actors]) => ({ group, ledgerTail, actors }));
  const entry: WarmupReadEntry = { promise: request, expiresAt: Number.POSITIVE_INFINITY };
  warmupReads.set(gid, entry);
  request.then(
    ({ group, ledgerTail, actors }) => {
      if (warmupReads.get(gid) !== entry) return;
      if (group.ok && ledgerTail.ok && actors.ok) {
        entry.expiresAt = Date.now() + COMPLETED_WARMUP_REUSE_MS;
      } else {
        warmupReads.delete(gid);
      }
    },
    () => {
      if (warmupReads.get(gid) === entry) warmupReads.delete(gid);
    },
  );
  return request;
}

export function resetGroupWarmupReadsForTests(): void {
  warmupReads.clear();
}
