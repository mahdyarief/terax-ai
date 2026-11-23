// Builds the remote agent for every Linux server the app can install it on,
// into src-tauri/binaries/terax-agent-<platform> (x86_64-linux-musl,
// aarch64-linux-musl), which the installer bundles
// (see tauri.nosign.conf.json). The agents are static musl binaries made
// with `cargo zigbuild`, so this works the same on Windows, macOS and Linux.
//
// Needs: `rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl`,
// `cargo install cargo-zigbuild`, and zig (on PATH, or `pip install ziglang`).
// TERAX_AGENT_PLATFORMS (comma-separated) builds a subset.
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  openSync,
  readSync,
  closeSync,
  statSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tauriDir = join(root, "src-tauri");
const ALL_PLATFORMS = ["x86_64-linux-musl", "aarch64-linux-musl"];
// Rust's name for each: cargo needs the full target triple.
const cargoTarget = (platform) => platform.replace("-linux-", "-unknown-linux-");

function fail(message) {
  process.stderr.write(`build-agent: ${message}\n`);
  process.exit(1);
}

function probe(command, args, env = process.env) {
  const result = spawnSync(command, args, { encoding: "utf8", env });
  return result.status === 0 ? (result.stdout ?? "").trim() : null;
}

function run(command, args, env) {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit", env });
  if (result.error) fail(`could not run ${command}: ${result.error.message}`);
  if (result.status !== 0) process.exit(result.status ?? 1);
}

// zig on PATH, or the one the `ziglang` Python package carries.
function zigEnv() {
  if (probe("zig", ["version"]) !== null) return process.env;
  for (const python of ["python", "python3", "py"]) {
    const dir = probe(python, [
      "-c",
      "import ziglang, os; print(os.path.dirname(ziglang.__file__))",
    ]);
    if (dir) {
      return { ...process.env, PATH: `${dir}${delimiter}${process.env.PATH ?? ""}` };
    }
  }
  fail("zig was not found: install it, or run `pip install ziglang`");
}

function isElf(path) {
  const fd = openSync(path, "r");
  try {
    const magic = Buffer.alloc(4);
    return readSync(fd, magic, 0, 4, 0) === 4 && magic.equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]));
  } finally {
    closeSync(fd);
  }
}

const requested = process.env.TERAX_AGENT_PLATFORMS?.trim();
const platforms = requested
  ? requested.split(",").map((p) => p.trim()).filter(Boolean)
  : ALL_PLATFORMS;
for (const platform of platforms) {
  if (!ALL_PLATFORMS.includes(platform)) fail(`unknown agent platform ${platform}`);
}
const targets = platforms.map(cargoTarget);

// `cargo zigbuild --version` would start a build; ask the binary itself.
if (probe("cargo-zigbuild", ["--version"]) === null) {
  fail("cargo-zigbuild was not found: run `cargo install cargo-zigbuild`");
}
const installed = probe("rustup", ["target", "list", "--installed"]);
if (installed !== null) {
  const missing = targets.filter((t) => !installed.split(/\s+/).includes(t));
  if (missing.length) fail(`missing Rust targets: run \`rustup target add ${missing.join(" ")}\``);
}

const env = zigEnv();
for (const platform of platforms) {
  const target = cargoTarget(platform);
  run(
    "cargo",
    [
      "zigbuild",
      "--release",
      "--locked",
      "--manifest-path",
      join(tauriDir, "Cargo.toml"),
      "--package",
      "terax-agent",
      "--bin",
      "terax-agent",
      "--target",
      target,
    ],
    env,
  );
  const source = join(tauriDir, "target", target, "release", "terax-agent");
  const destination = join(tauriDir, "binaries", `terax-agent-${platform}`);
  try {
    if (!statSync(source).isFile() || !isElf(source)) fail(`not a Linux executable: ${source}`);
  } catch (error) {
    fail(`missing build output ${source}: ${error.message}`);
  }
  mkdirSync(dirname(destination), { recursive: true });
  copyFileSync(source, destination);
  chmodSync(destination, 0o755);
  const size = (statSync(destination).size / 1048576).toFixed(1);
  console.log(`Prepared ${destination.slice(root.length + 1)} (${size} MiB)`);
}
