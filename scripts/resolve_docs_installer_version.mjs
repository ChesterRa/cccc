import { readFile } from "node:fs/promises";

const metadataFlag = process.argv.indexOf("--metadata");
const metadataPath = metadataFlag >= 0 ? process.argv[metadataFlag + 1] : "";

if (metadataFlag >= 0 && !metadataPath) {
  throw new Error("--metadata requires a JSON file path");
}

function requiredAssets(version) {
  return [
    `cccc-v${version}-aarch64-apple-darwin.tar.gz`,
    `cccc-v${version}-x86_64-apple-darwin.tar.gz`,
    `cccc-v${version}-x86_64-pc-windows-msvc.zip`,
    `cccc-v${version}-x86_64-unknown-linux-gnu.tar.gz`,
    "SHA256SUMS",
    "install.ps1",
    "install.sh",
  ];
}

function parseVersion(value) {
  const match = /^v?([0-9]+)\.([0-9]+)\.([0-9]+)(?:-([0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*))?(?:\+([0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*))?$/.exec(
    value || "",
  );
  if (!match) return null;
  return {
    raw: match[0].replace(/^v/, ""),
    core: [Number(match[1]), Number(match[2]), Number(match[3])],
    prerelease: match[4] ? match[4].split(".") : [],
  };
}

function compareIdentifiers(left, right) {
  const leftNumeric = /^[0-9]+$/.test(left);
  const rightNumeric = /^[0-9]+$/.test(right);
  if (leftNumeric && rightNumeric) return Number(left) - Number(right);
  if (leftNumeric !== rightNumeric) return leftNumeric ? -1 : 1;
  const leftNatural = /^([A-Za-z-]+)([0-9]+)$/.exec(left);
  const rightNatural = /^([A-Za-z-]+)([0-9]+)$/.exec(right);
  if (leftNatural && rightNatural && leftNatural[1] === rightNatural[1]) {
    return Number(leftNatural[2]) - Number(rightNatural[2]);
  }
  return left.localeCompare(right, "en");
}

function compareVersions(left, right) {
  for (let index = 0; index < left.core.length; index += 1) {
    if (left.core[index] !== right.core[index]) return left.core[index] - right.core[index];
  }
  if (left.prerelease.length === 0 || right.prerelease.length === 0) {
    if (left.prerelease.length === right.prerelease.length) return 0;
    return left.prerelease.length === 0 ? 1 : -1;
  }
  const length = Math.max(left.prerelease.length, right.prerelease.length);
  for (let index = 0; index < length; index += 1) {
    const leftIdentifier = left.prerelease[index];
    const rightIdentifier = right.prerelease[index];
    if (leftIdentifier === undefined || rightIdentifier === undefined) {
      return leftIdentifier === undefined ? -1 : 1;
    }
    const compared = compareIdentifiers(leftIdentifier, rightIdentifier);
    if (compared !== 0) return compared;
  }
  return 0;
}

function completeReleaseVersion(release) {
  const parsed = parseVersion(release.tag_name);
  if (!String(release.tag_name || "").startsWith("v") || !parsed || release.draft) {
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
  const repository = process.env.GITHUB_REPOSITORY || "ChesterRa/cccc";
  const headers = {
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
    "User-Agent": "cccc-docs-release-resolver",
  };
  if (process.env.GITHUB_TOKEN) {
    headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  }
  const response = await fetch(`https://api.github.com/repos/${repository}/releases?per_page=100`, {
    headers,
  });
  if (!response.ok) {
    throw new Error(`Could not list GitHub Releases (${response.status})`);
  }
  releases = await response.json();
}

const version = releases
  .map(completeReleaseVersion)
  .filter(Boolean)
  .map((value) => parseVersion(value))
  .filter(Boolean)
  .sort((left, right) => compareVersions(right, left))[0]?.raw;
if (!version) {
  throw new Error("No published GitHub Release has the complete installer asset set");
}

console.log(version);
