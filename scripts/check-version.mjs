import { readFileSync } from "node:fs";

const json = (path) => JSON.parse(readFileSync(new URL(`../${path}`, import.meta.url), "utf8"));
const version = json("package.json").version;
const lock = json("package-lock.json");
const cargo = readFileSync(new URL("../src-tauri/Cargo.toml", import.meta.url), "utf8");
const cargoLock = readFileSync(new URL("../src-tauri/Cargo.lock", import.meta.url), "utf8");
const versions = {
  "package-lock.json": lock.version,
  "package-lock.json root": lock.packages[""].version,
  "tauri.conf.json": json("src-tauri/tauri.conf.json").version,
  "Cargo.toml": cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1],
  "Cargo.lock": cargoLock.match(/\[\[package\]\]\s+name = "zapret-gui"\s+version = "([^"]+)"/)?.[1],
};
for (const [file, value] of Object.entries(versions)) {
  if (value !== version) throw new Error(`${file}: ${value} differs from ${version}`);
}
const tag = process.env.GITHUB_REF_TYPE === "tag" ? process.env.GITHUB_REF_NAME?.replace(/^v/, "") : null;
if (tag && tag !== version) throw new Error(`Tag ${tag} differs from ${version}`);
console.log(`All application versions match: ${version}`);
