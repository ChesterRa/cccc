import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "groupStoreAsyncActions.ts"), "utf8");

describe("group store request triggers", () => {
  it("keeps internal runtime actor refresh eligible for shared read dedupe", () => {
    expect(source).not.toContain("api.fetchActors(gid, false, { noCache: true }, { includeInternal: true })");
  });
});
