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

function completeReleaseVersion(release) {
  const match = /^v([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?)$/.exec(
    release.tag_name || "",
  );
  if (!match || release.draft) {
    return "";
  }
  const version = match[1];
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

const version = releases.map(completeReleaseVersion).find(Boolean);
if (!version) {
  throw new Error("No published GitHub Release has the complete installer asset set");
}

console.log(version);
