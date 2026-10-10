// `npm run dev`: starts the backend and the page, opens the browser, and
// rebuilds and restarts the backend when Rust code or a profile changes.

import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { watch } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const binary = process.platform === "win32" ? "unimcp.exe" : "unimcp";
const unimcp = join(root, "target", "debug", binary);
const next = join("node_modules", "next", "dist", "bin", "next");

// One key for the whole session, so the open page survives backend restarts.
const key = randomBytes(16).toString("hex");
const url = `http://localhost:3000/?key=${key}`;

let backend = null;
let rebuilding = false;
let changedMeanwhile = false;

function exited(child) {
  return new Promise((resolve) => child.once("exit", resolve));
}

async function startBackend() {
  const build = spawn("cargo", ["build", "--quiet"], { cwd: root, stdio: "inherit" });
  if ((await exited(build)) !== 0) {
    console.log("\n[dev] The backend did not compile. Fix the error and save again.\n");
    return;
  }
  backend = spawn(unimcp, ["setup", "--dev"], {
    stdio: "inherit",
    env: { ...process.env, UNIMCP_DEV_KEY: key },
  });
}

async function restartBackend() {
  if (rebuilding) {
    changedMeanwhile = true;
    return;
  }
  rebuilding = true;
  do {
    changedMeanwhile = false;
    console.log("\n[dev] Rust code changed, restarting the backend...\n");
    // Windows cannot replace a program while it is running.
    if (backend && backend.exitCode === null) {
      backend.kill();
      await exited(backend);
    }
    await startBackend();
  } while (changedMeanwhile);
  rebuilding = false;
}

function openBrowser() {
  const [command, args] =
    process.platform === "win32"
      ? ["cmd", ["/c", "start", "", url]]
      : process.platform === "darwin"
        ? ["open", [url]]
        : ["xdg-open", [url]];
  spawn(command, args, { stdio: "ignore", detached: true }).unref();
}

await startBackend();

const page = spawn(process.execPath, [next, "dev", "-H", "localhost"], {
  stdio: ["inherit", "pipe", "inherit"],
});
let opened = false;
page.stdout.on("data", (chunk) => {
  process.stdout.write(chunk);
  if (!opened && chunk.toString().includes("Ready")) {
    opened = true;
    console.log(`\n[dev] Page: ${url}\n`);
    if (!process.argv.includes("--no-open")) openBrowser();
  }
});

let settle;
for (const folder of ["crates", "profiles"]) {
  watch(join(root, folder), { recursive: true }, () => {
    // Editors write a file in several steps.
    clearTimeout(settle);
    settle = setTimeout(restartBackend, 300);
  });
}

function stop() {
  backend?.kill();
  page.kill();
  process.exit();
}
process.on("SIGINT", stop);
process.on("SIGTERM", stop);
page.once("exit", stop);
