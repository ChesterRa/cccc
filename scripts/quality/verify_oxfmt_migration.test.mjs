import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  countPythonSplitlines,
  sha256,
  verifyImmutableManifest,
  verifyInitialMigration,
} from "./oxfmt_migration_contract.mjs";
import { migrationEntries } from "./verify_oxfmt_migration.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const VERIFIER_URL = pathToFileURL(path.join(ROOT, "scripts/quality/verify_oxfmt_migration.mjs"));
const FORMATTER_VERSION = "0.57.0";
const PATH = "web/src/example.ts";
const CURRENT = Buffer.from("line\n".repeat(330));
const ENTRY = {
  path: PATH,
  baseBlobOid: "a".repeat(40),
  formattedSha256: sha256(CURRENT),
  baseLines: 320,
  formattedLines: countPythonSplitlines(CURRENT),
};

function verify(overrides = {}) {
  const manifest = overrides.manifest ?? {
    version: 1,
    formatter: { name: "oxfmt", version: FORMATTER_VERSION },
    files: [ENTRY],
  };
  verifyInitialMigration({
    manifest,
    formatterVersion: FORMATTER_VERSION,
    expectedEntries: overrides.expectedEntries ?? [ENTRY],
    currentBaseline: overrides.currentBaseline ?? { [PATH]: 330 },
    baseBaseline: { [PATH]: 320 },
    currentFiles: overrides.currentFiles ?? new Map([[PATH, CURRENT]]),
    limit: 300,
  });
}

test("rejects current bytes changed without changing the line count", () => {
  const changed = Buffer.from("xxxx\n".repeat(330));
  assert.throws(() => verify({ currentFiles: new Map([[PATH, changed]]) }), /current hash changed/);
});

test("rejects a baseline raised above the exact formatted line count", () => {
  assert.throws(() => verify({ currentBaseline: { [PATH]: 331 } }), /baseline is not exact/);
});

test("rejects an incomplete deterministic manifest", () => {
  const second = { ...ENTRY, path: "web/src/second.ts", baseBlobOid: "b".repeat(40) };
  assert.throws(() => verify({ expectedEntries: [ENTRY, second] }), /complete deterministic/);
});

test("rejects changes after the manifest exists in the trusted base", () => {
  assert.throws(
    () => verifyImmutableManifest(Buffer.from("changed\n"), Buffer.from("original\n")),
    /byte-for-byte unchanged/,
  );
});

test("rejects an invalid explicit base through the real CLI", () => {
  const result = spawnSync(
    process.execPath,
    ["scripts/quality/verify_oxfmt_migration.mjs", "--base-ref", "not-a-real-base"],
    { cwd: ROOT, encoding: "utf8" },
  );
  assert.equal(result.status, 1);
  assert.match(result.stderr, /not-a-real-base/);
});

test("removes the temporary source tree when the formatter fails", () => {
  assert.equal(typeof migrationEntries, "function");
  const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "cccc-oxfmt-cleanup-test-"));
  const failingFormatter = path.join(sandbox, "fail.mjs");
  fs.writeFileSync(failingFormatter, "process.exit(7);\n");
  const source = [
    `import { migrationEntries } from ${JSON.stringify(VERIFIER_URL.href)};`,
    `migrationEntries("HEAD", ${JSON.stringify(failingFormatter)});`,
  ].join("\n");

  try {
    const result = spawnSync(process.execPath, ["--input-type=module", "--eval", source], {
      cwd: ROOT,
      encoding: "utf8",
      env: { ...process.env, TMPDIR: sandbox, TEMP: sandbox, TMP: sandbox },
    });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /exited 7/);
    assert.deepEqual(fs.readdirSync(sandbox), ["fail.mjs"]);
  } finally {
    fs.rmSync(sandbox, { recursive: true, force: true });
  }
});
