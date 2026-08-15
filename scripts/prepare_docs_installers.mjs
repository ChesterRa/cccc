import { chmod, mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const publicDir = join(root, "docs", "public");
const cargoManifest = await readFile(join(root, "Cargo.toml"), "utf8");
const version =
  process.env.CCCC_DOCS_INSTALL_VERSION || cargoManifest.match(/^version = "([^"]+)"$/m)?.[1];

if (!version) {
  throw new Error("Could not read the CCCC version from Cargo.toml");
}

await mkdir(publicDir, { recursive: true });
await Promise.all(
  ["install.sh", "install.ps1"].map(async (name) => {
    const template = await readFile(join(root, "scripts", name), "utf8");
    const rendered = template
      .replaceAll("@CCCC_VERSION@", version)
      .replaceAll("@CCCC_RELEASE_TAG_PREFIX@", "v");
    if (rendered.includes("@CCCC_")) {
      throw new Error(`Unrendered installer metadata in ${name}`);
    }
    await writeFile(join(publicDir, name), rendered);
  }),
);
await chmod(join(publicDir, "install.sh"), 0o755);
