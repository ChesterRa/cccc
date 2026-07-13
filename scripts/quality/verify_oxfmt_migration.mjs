#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  countPythonSplitlines,
  sha256,
  verifyImmutableManifest,
  verifyInitialMigration,
} from "./oxfmt_migration_contract.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const MANIFEST_PATH = "scripts/quality/oxfmt-migration-v1.json";
const BASELINE_PATH = "scripts/quality/source-size-baseline.json";
const LOCK_PATH = "web/package-lock.json";
const OXFMT_BIN_PATH = "web/node_modules/oxfmt/bin/oxfmt";
const VITE_PLUS_BIN_PATH = "web/node_modules/vite-plus/bin/vp";
const OXFMT_VERSION = "0.57.0";
const VITE_PLUS_VERSION = "0.2.4";
const LIMIT = 300;

class VerificationError extends Error {}

function comparePaths(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function fail(message) {
  throw new VerificationError(message);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? ROOT,
    encoding: Object.hasOwn(options, "encoding") ? options.encoding : "utf8",
    env: { ...process.env, ...options.env },
    input: options.input,
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.status !== 0 && !options.allowFailure) {
    fail(`${command} ${args.join(" ")} exited ${result.status}: ${String(result.stderr).trim()}`);
  }
  return result;
}

function gitText(...args) {
  return run("git", args).stdout.trim();
}

function gitBytes(ref, relative, allowMissing = false) {
  const result = run("git", ["show", `${ref}:${relative}`], {
    encoding: null,
    allowFailure: allowMissing,
  });
  return result.status === 0 ? result.stdout : null;
}

function resolveBaseRef(explicit) {
  if (explicit) return gitText("rev-parse", "--verify", `${explicit}^{commit}`);
  const head = gitText("rev-parse", "--verify", "HEAD^{commit}");
  const upstream = run(
    "git",
    ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"],
    { allowFailure: true },
  ).stdout.trim();
  for (const candidate of [upstream, "origin/main", "origin/master"]) {
    if (!candidate) continue;
    const commit = run("git", ["rev-parse", "--verify", `${candidate}^{commit}`], {
      allowFailure: true,
    });
    if (commit.status !== 0) continue;
    const mergeBase = run("git", ["merge-base", head, commit.stdout.trim()], {
      allowFailure: true,
    });
    if (mergeBase.status === 0 && mergeBase.stdout.trim()) return mergeBase.stdout.trim();
  }
  fail("cannot resolve a trusted base; pass --base-ref <commit>");
}

function loadJson(relative) {
  return JSON.parse(fs.readFileSync(path.join(ROOT, relative), "utf8"));
}

function loadBaseBaseline(baseRef, baseSources) {
  const baselineBytes = gitBytes(baseRef, BASELINE_PATH, true);
  if (baselineBytes !== null) return JSON.parse(baselineBytes.toString("utf8")).files;

  const files = {};
  for (const [relative, { bytes }] of baseSources) {
    const lines = countPythonSplitlines(bytes);
    if (lines > LIMIT) files[relative] = lines;
  }
  return files;
}

function assertFormatterVersion() {
  const lock = loadJson(LOCK_PATH);
  const lockedOxfmt = lock.packages?.["node_modules/oxfmt"]?.version;
  const lockedVitePlus = lock.packages?.["node_modules/vite-plus"]?.version;
  if (lockedOxfmt !== OXFMT_VERSION) {
    fail(`lockfile must contain oxfmt ${OXFMT_VERSION}, got ${lockedOxfmt ?? "missing"}`);
  }
  if (lockedVitePlus !== VITE_PLUS_VERSION) {
    fail(
      `lockfile must contain vite-plus ${VITE_PLUS_VERSION}, got ${lockedVitePlus ?? "missing"}`,
    );
  }
  const oxfmtBinary = path.join(ROOT, OXFMT_BIN_PATH);
  const vpBinary = path.join(ROOT, VITE_PLUS_BIN_PATH);
  if (!fs.existsSync(oxfmtBinary) || !fs.existsSync(vpBinary)) {
    fail("web dependencies are not installed; run npm ci in web");
  }
  const actualOxfmt = run(process.execPath, [oxfmtBinary, "--version"]).stdout.match(
    /\d+\.\d+\.\d+/,
  )?.[0];
  const actualVitePlus = run(process.execPath, [vpBinary, "--version"], {
    cwd: path.join(ROOT, "web"),
  }).stdout.match(/\d+\.\d+\.\d+/)?.[0];
  if (actualOxfmt !== OXFMT_VERSION) {
    fail(`oxfmt binary is ${actualOxfmt ?? "unknown"}, expected ${OXFMT_VERSION}`);
  }
  if (actualVitePlus !== VITE_PLUS_VERSION) {
    fail(`vp binary is ${actualVitePlus ?? "unknown"}, expected ${VITE_PLUS_VERSION}`);
  }
  return vpBinary;
}

function isProductionSource(relative) {
  if (!relative.startsWith("web/src/") || !/\.(ts|tsx)$/.test(relative)) return false;
  const parts = relative.split("/");
  if (
    parts.some((part) => ["dist", "generated", "node_modules", "tests", "vendor"].includes(part))
  ) {
    return false;
  }
  return !/\.(test|spec)\.(ts|tsx)$/.test(relative);
}

function readBatchBlobs(tree) {
  const result = run("git", ["cat-file", "--batch"], {
    encoding: null,
    input: Buffer.from(`${tree.map(({ oid }) => oid).join("\n")}\n`),
  });
  const output = result.stdout;
  const sources = new Map();
  let offset = 0;
  for (const { oid, relative } of tree) {
    const headerEnd = output.indexOf(0x0a, offset);
    if (headerEnd < 0) fail(`missing cat-file header for ${relative}`);
    const [actualOid, kind, rawSize] = output.subarray(offset, headerEnd).toString("utf8").split(" ");
    const size = Number(rawSize);
    const contentStart = headerEnd + 1;
    const contentEnd = contentStart + size;
    if (actualOid !== oid || kind !== "blob" || !Number.isSafeInteger(size)) {
      fail(`invalid cat-file header for ${relative}`);
    }
    if (output[contentEnd] !== 0x0a) fail(`invalid cat-file framing for ${relative}`);
    sources.set(relative, { oid, bytes: Buffer.from(output.subarray(contentStart, contentEnd)) });
    offset = contentEnd + 1;
  }
  return sources;
}

export function migrationEntries(baseRef, vpBinary) {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "cccc-oxfmt-v1-"));
  try {
    const tree = gitText("ls-tree", "-r", baseRef, "--", "web/src")
      .split("\n")
      .filter(Boolean)
      .map((line) => {
        const match = line.match(/^\d+ blob ([0-9a-f]+)\t(.+)$/);
        if (!match) fail(`cannot parse git ls-tree output: ${line}`);
        return { oid: match[1], relative: match[2] };
      })
      .filter(({ relative }) => isProductionSource(relative))
      .sort((left, right) => comparePaths(left.relative, right.relative));
    const baseSources = readBatchBlobs(tree);
    for (const [relative, { bytes }] of baseSources) {
      const destination = path.join(tempRoot, relative);
      fs.mkdirSync(path.dirname(destination), { recursive: true });
      fs.writeFileSync(destination, bytes);
    }
    const baseBaseline = loadBaseBaseline(baseRef, baseSources);
    const paths = [...baseSources.keys()];
    const tempWeb = path.join(tempRoot, "web");
    for (const metadata of ["vite.config.ts", "package.json", "package-lock.json", "tsconfig.json"]) {
      fs.copyFileSync(path.join(ROOT, "web", metadata), path.join(tempWeb, metadata));
    }
    fs.symlinkSync(
      path.join(ROOT, "web/node_modules"),
      path.join(tempWeb, "node_modules"),
      process.platform === "win32" ? "junction" : "dir",
    );
    run(process.execPath, [vpBinary, "fmt", "src", "--write"], { cwd: tempWeb });

    const entries = [];
    for (const relative of paths) {
      const currentPath = path.join(ROOT, relative);
      if (!fs.existsSync(currentPath)) continue;
      const { oid, bytes: baseBytes } = baseSources.get(relative);
      const formattedBytes = fs.readFileSync(path.join(tempRoot, relative));
      const currentBytes = fs.readFileSync(currentPath);
      if (!currentBytes.equals(formattedBytes)) continue;

      const baseLines = countPythonSplitlines(baseBytes);
      const formattedLines = countPythonSplitlines(formattedBytes);
      const oldLimit = baseBaseline[relative];
      const raisesExisting =
        oldLimit !== undefined && oldLimit === baseLines && formattedLines > oldLimit;
      const crossesLimit = oldLimit === undefined && baseLines <= LIMIT && formattedLines > LIMIT;
      if (!raisesExisting && !crossesLimit) continue;

      entries.push({
        path: relative,
        baseBlobOid: oid,
        formattedSha256: sha256(formattedBytes),
        baseLines,
        formattedLines,
      });
    }
    return entries;
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
}

function writeMigration(baseRef, binary) {
  if (gitBytes(baseRef, MANIFEST_PATH, true) !== null) {
    fail("the trusted base already contains v1; create a new version instead");
  }
  const files = migrationEntries(baseRef, binary);
  if (files.length === 0) fail("no formatter-only source-size migrations were found");
  const manifest = { version: 1, formatter: { name: "oxfmt", version: OXFMT_VERSION }, files };
  const baseline = loadJson(BASELINE_PATH);
  const existingManifestPath = path.join(ROOT, MANIFEST_PATH);
  if (fs.existsSync(existingManifestPath)) {
    const existingManifest = JSON.parse(fs.readFileSync(existingManifestPath, "utf8"));
    for (const entry of existingManifest.files ?? []) {
      if (entry.baseLines > LIMIT) baseline.files[entry.path] = entry.baseLines;
      else delete baseline.files[entry.path];
    }
  }
  for (const entry of files) baseline.files[entry.path] = entry.formattedLines;
  baseline.files = Object.fromEntries(
    Object.entries(baseline.files).sort(([left], [right]) => comparePaths(left, right)),
  );
  fs.writeFileSync(existingManifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  fs.writeFileSync(path.join(ROOT, BASELINE_PATH), `${JSON.stringify(baseline, null, 2)}\n`);
  console.log(`Wrote ${MANIFEST_PATH} with ${files.length} formatter-only entries.`);
}

function verifyMigration(baseRef, binary) {
  const manifestBytes = fs.readFileSync(path.join(ROOT, MANIFEST_PATH));
  const baseManifestBytes = gitBytes(baseRef, MANIFEST_PATH, true);
  if (baseManifestBytes !== null) {
    try {
      verifyImmutableManifest(manifestBytes, baseManifestBytes);
    } catch (error) {
      fail(error.message);
    }
    console.log(`Oxfmt migration v1 is immutable relative to base ${baseRef.slice(0, 12)}.`);
    return;
  }

  const manifest = JSON.parse(manifestBytes.toString("utf8"));
  const expectedEntries = migrationEntries(baseRef, binary);
  const baseBaseline = Object.fromEntries(
    manifest.files
      .filter((entry) => entry.baseLines > LIMIT)
      .map((entry) => [entry.path, entry.baseLines]),
  );
  const currentBaseline = loadJson(BASELINE_PATH).files;
  const currentFiles = new Map(
    manifest.files.map((entry) => [entry.path, fs.readFileSync(path.join(ROOT, entry.path))]),
  );
  try {
    verifyInitialMigration({
      manifest,
      formatterVersion: OXFMT_VERSION,
      expectedEntries,
      currentBaseline,
      baseBaseline,
      currentFiles,
      limit: LIMIT,
    });
  } catch (error) {
    fail(error.message);
  }
  console.log(
    `Verified ${manifest.files.length} Oxfmt ${OXFMT_VERSION} formatter-only migrations against base ${baseRef.slice(0, 12)}.`,
  );
}

export function main(args = process.argv.slice(2)) {
  const baseIndex = args.indexOf("--base-ref");
  const explicitBase = baseIndex >= 0 ? args[baseIndex + 1] : "";
  if (baseIndex >= 0 && !explicitBase) fail("--base-ref requires a commit");
  const write = args.includes("--write-migration");
  const knownArgs = new Set(["--base-ref", explicitBase, "--write-migration"]);
  for (const arg of args) if (!knownArgs.has(arg)) fail(`unknown argument ${arg}`);

  const baseRef = resolveBaseRef(explicitBase);
  const binary = assertFormatterVersion();
  if (write) writeMigration(baseRef, binary);
  verifyMigration(baseRef, binary);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    main();
  } catch (error) {
    if (!(error instanceof VerificationError)) throw error;
    console.error(`Oxfmt migration verification failed: ${error.message}`);
    process.exitCode = 1;
  }
}
