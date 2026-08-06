import { chmod, copyFile, mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const publicDir = join(root, "docs", "public");

await mkdir(publicDir, { recursive: true });
await Promise.all(
  ["install.sh", "install.ps1"].map((name) =>
    copyFile(join(root, "scripts", name), join(publicDir, name)),
  ),
);
await chmod(join(publicDir, "install.sh"), 0o755);
