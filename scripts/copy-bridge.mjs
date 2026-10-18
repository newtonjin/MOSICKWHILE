import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(root, "target", "release", "mosickwhile_bridge.dll");
const dests = [
  join(root, "src-tauri", "mosickwhile_bridge.dll"),
  join(root, "dist-release", "mosickwhile_bridge.dll"),
];

if (!existsSync(src)) {
  console.error("Bridge DLL missing:", src);
  process.exit(1);
}

for (const dest of dests) {
  mkdirSync(dirname(dest), { recursive: true });
  copyFileSync(src, dest);
  console.log("Copied bridge →", dest);
}
