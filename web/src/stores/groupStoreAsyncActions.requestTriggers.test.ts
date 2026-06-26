import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "groupStoreAsyncActions.ts"), "utf8");

describe("group store request triggers", () => {
  it("keeps internal runtime actor refresh eligible for shared read dedupe", () => {
    expect(source).not.toContain("api.fetchActors(gid, false, { noCache: true }, { includeInternal: true })");
  });

  it("cancels stale group load requests when switching groups", () => {
    expect(source).toContain("abortStaleLoadGroupRequests(gid)");
    expect(source).toContain("const loadSignal = loadController.signal;");
    expect(source).toContain("loadGroupInFlight.delete(groupId)");
    expect(source).toContain("api.fetchGroup(gid, { signal: loadSignal })");
    expect(source).toContain("api.fetchLedgerTail(gid, INITIAL_LEDGER_TAIL_LIMIT, { includeStatuses: false, signal: loadSignal })");
    expect(source).toContain("api.fetchActors(gid, false, { signal: loadSignal }, { includeInternal: true })");
  });

  it("delays cached tail refresh until after the first cached render", () => {
    expect(source).toContain("shouldDeferInitialTailRefresh(chatBucket)");
    expect(source).toContain("CACHED_TAIL_REFRESH_DELAY_MS");
  });
});
