import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

const metadataFlag = process.argv.indexOf("--metadata");
const metadataPath = metadataFlag >= 0 ? process.argv[metadataFlag + 1] : "";
const outputFlag = process.argv.indexOf("--output");
const outputPath = outputFlag >= 0 ? process.argv[outputFlag + 1] : "";
const repository = process.env.GITHUB_REPOSITORY || "ChesterRa/cccc";

if (metadataFlag >= 0 && !metadataPath) {
  throw new Error("--metadata requires a JSON file path");
}
if (outputFlag >= 0 && !outputPath) {
  throw new Error("--output requires a JSON file path");
}
if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
  throw new Error("GITHUB_REPOSITORY must use the owner/repository form");
}

function requiredAssets(version) {
  return [
    `cccc-v${version}-aarch64-apple-darwin.tar.gz`,
    `cccc-v${version}-x86_64-pc-windows-msvc.zip`,
    `cccc-v${version}-x86_64-unknown-linux-gnu.tar.gz`,
    "SHA256SUMS",
    "install.ps1",
    "install.sh",
  ];
}

function parseVersion(value) {
  const match = /^v?(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/.exec(
    value || "",
  );
  if (!match) return null;
  const core = [BigInt(match[1]), BigInt(match[2]), BigInt(match[3])];
  const prerelease = match[4] ? match[4].split(".") : [];
  if (core.some((part) => part > 18446744073709551615n) ||
      prerelease.some((part) => /^0[0-9]+$/.test(part))) return null;
  // CCCC historically tags prereleases as rc2/rc10, not rc.2/rc.10.
  const sequence = /^(alpha|beta|rc)(0|[1-9][0-9]*)$/.exec(match[4] || "");
  return {
    raw: match[0].replace(/^v/, ""),
    core,
    prerelease: sequence ? [sequence[1], sequence[2]] : prerelease,
  };
}

function compareVersions(left, right) {
  for (let index = 0; index < left.core.length; index += 1) {
    if (left.core[index] !== right.core[index]) return left.core[index] < right.core[index] ? -1 : 1;
  }
  if (Boolean(left.prerelease.length) !== Boolean(right.prerelease.length)) {
    return Number(!left.prerelease.length) - Number(!right.prerelease.length);
  }
  for (let index = 0; index < Math.max(left.prerelease.length, right.prerelease.length); index += 1) {
    const a = left.prerelease[index];
    const b = right.prerelease[index];
    if (a === b) continue;
    if (a === undefined || b === undefined) return a === undefined ? -1 : 1;
    const numericA = /^[0-9]+$/.test(a);
    const numericB = /^[0-9]+$/.test(b);
    if (numericA && numericB) return BigInt(a) < BigInt(b) ? -1 : 1;
    if (numericA !== numericB) return numericA ? -1 : 1;
    return a < b ? -1 : 1;
  }
  // Build metadata does not change precedence; break ties deterministically.
  if (left.raw !== right.raw) return left.raw < right.raw ? -1 : 1;
  return 0;
}

function completeReleaseVersion(release) {
  const parsed = parseVersion(release.tag_name);
  if (
    !String(release.tag_name || "").startsWith("v") ||
    !parsed ||
    release.draft !== false ||
    release.prerelease !== (parsed.prerelease.length > 0)
  ) {
    return "";
  }
  const version = parsed.raw;
  const uploadedAssets = new Set(
    (release.assets || [])
      .filter((asset) => asset.state === "uploaded")
      .map((asset) => asset.name),
  );
  return requiredAssets(version).every((name) => uploadedAssets.has(name)) ? version : "";
}

let releases;
if (metadataPath) {
  const metadata = JSON.parse(await readFile(metadataPath, "utf8"));
  releases = Array.isArray(metadata) ? metadata : [metadata];
} else {
  const headers = {
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
    "User-Agent": "cccc-docs-release-resolver",
  };
  if (process.env.GITHUB_TOKEN) {
    headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  }
  releases = [];
  for (let page = 1; ; page += 1) {
    const response = await fetch(`https://api.github.com/repos/${repository}/releases?per_page=100&page=${page}`, {
      headers,
      signal: AbortSignal.timeout(30_000),
    });
    if (!response.ok) {
      throw new Error(`Could not list GitHub Releases (${response.status})`);
    }
    const batch = await response.json();
    if (!Array.isArray(batch)) throw new Error("GitHub Releases response must be an array");
    releases.push(...batch);
    if (!response.headers.get("link")?.includes('rel="next"')) break;
  }
}

const complete = releases
  .map(completeReleaseVersion)
  .filter(Boolean)
  .map((value) => parseVersion(value))
  .filter(Boolean)
  .sort((left, right) => compareVersions(right, left));
const version = complete.find((release) => !release.prerelease.length)?.raw;
if (!version) {
  throw new Error("No published stable GitHub Release has the complete installer asset set");
}

if (outputPath) {
  const index = {
    schema_version: 1,
    repository,
    channels: {
      stable: version,
      rc: complete.find((release) => release.prerelease.length)?.raw ?? null,
    },
  };
  await mkdir(dirname(outputPath), { recursive: true });
  const temporary = `${outputPath}.${process.pid}.tmp`;
  try {
    await writeFile(temporary, `${JSON.stringify(index, null, 2)}\n`);
    await rename(temporary, outputPath);
  } finally {
    await rm(temporary, { force: true });
  }
}

console.log(version);
