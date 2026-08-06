import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const webDir = join(root, "web");
const webDist = join(webDir, "dist");
const rustAssets = join(root, "crates", "cccc-web", "assets", "web-dist");
const npm = process.platform === "win32" ? "npm.cmd" : "npm";

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
    shell: process.platform === "win32",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

if (process.argv.includes("--install-deps")) {
  run(npm, ["ci", "--prefix", webDir]);
}
run(npm, ["-C", webDir, "run", "build"]);

if (!existsSync(join(webDist, "index.html"))) {
  throw new Error(`Rust Web build did not produce ${join(webDist, "index.html")}`);
}

rmSync(rustAssets, { recursive: true, force: true });
mkdirSync(rustAssets, { recursive: true });
cpSync(webDist, rustAssets, { recursive: true });

console.log("OK: prepared Rust crate Web assets -> crates/cccc-web/assets/web-dist");
