// 5d packaging: turn CI build artifacts into the per-platform npm
// sub-packages declared as optionalDependencies of the main package.
//
// Input: a directory of artifacts named `<triple>.node`
// (e.g. darwin-arm64.node, linux-x64-gnu.node). Output: npm/<triple>/
// with a package.json (os/cpu/libc gated so npm installs only the
// matching one) + the binary as `shm-native.node`.
//
// Usage: node scripts/prepare-packages.mjs <artifacts-dir> [version]
import { readdirSync, mkdirSync, copyFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const [artDir, version = "0.0.1"] = process.argv.slice(2);
if (!artDir) {
  console.error("usage: prepare-packages.mjs <artifacts-dir> [version]");
  process.exit(1);
}

// triple → npm os/cpu/libc gate (npm refuses to install a mismatched one).
const GATE = {
  "darwin-arm64": { os: ["darwin"], cpu: ["arm64"] },
  "darwin-x64": { os: ["darwin"], cpu: ["x64"] },
  "linux-x64-gnu": { os: ["linux"], cpu: ["x64"], libc: ["glibc"] },
  "linux-x64-musl": { os: ["linux"], cpu: ["x64"], libc: ["musl"] },
  "linux-arm64-gnu": { os: ["linux"], cpu: ["arm64"], libc: ["glibc"] },
  "win32-x64-msvc": { os: ["win32"], cpu: ["x64"] },
};

let made = 0;
for (const f of readdirSync(artDir)) {
  if (!f.endsWith(".node")) continue;
  const triple = f.replace(/\.node$/, "");
  const gate = GATE[triple];
  if (!gate) {
    console.error(`skip unknown triple: ${triple}`);
    continue;
  }
  const dir = join(root, "npm", triple);
  mkdirSync(dir, { recursive: true });
  copyFileSync(join(artDir, f), join(dir, "shm-native.node"));
  writeFileSync(
    join(dir, "package.json"),
    `${JSON.stringify(
      {
        name: `@rstui-acp/plugin-shm-native-${triple}`,
        version,
        description: `shm-native prebuilt binary for ${triple} (ADR 0019).`,
        license: "Apache-2.0",
        main: "shm-native.node",
        files: ["shm-native.node"],
        ...gate,
      },
      null,
      2,
    )}\n`,
  );
  made++;
  console.log(`prepared npm/${triple}`);
}
console.log(`${made} platform package(s) ready under sdk/shm-native/npm/`);
