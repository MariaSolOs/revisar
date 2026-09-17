#!/usr/bin/env node
// Register this checkout locally; never copy its implementation into Pi config.
import {
    mkdir,
    readFile,
    realpath,
    rename,
    rm,
    writeFile,
} from "node:fs/promises";
import { randomUUID } from "node:crypto";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const marker = "// revisar Pi loader (managed by scripts/setup-pi.mjs).\n";

async function atomicWrite(file, contents) {
    await mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
    const pending = `${file}.${randomUUID()}.pending`;
    try {
        await writeFile(pending, contents, { flag: "wx", mode: 0o600 });
        await rename(pending, file);
    } finally {
        await rm(pending, { force: true });
    }
}

async function setup() {
    if (process.argv.length !== 2) {
        throw new Error("Usage: node scripts/setup-pi.mjs (no arguments)");
    }
    // Resolve from the script, not cwd. Symlinked checkouts and paths containing
    // spaces, quotes, Unicode, or newlines are stored without shell evaluation.
    const checkout = await realpath(
        fileURLToPath(new URL("../", import.meta.url)),
    );
    const source = path.join(checkout, "integrations", "pi", "loader.ts");
    const loader = await readFile(source, "utf8");
    await readFile(
        path.join(checkout, "integrations", "pi", "index.ts"),
        "utf8",
    );
    if (!loader.startsWith(marker))
        throw new Error("Unrecognized loader template");

    const agentDir = process.env.PI_CODING_AGENT_DIR
        ? path.resolve(
              process.env.PI_CODING_AGENT_DIR.replace(
                  /^~(?=\/|$)/,
                  os.homedir(),
              ),
          )
        : path.join(os.homedir(), ".pi", "agent");
    const stateHome = path.isAbsolute(process.env.XDG_STATE_HOME ?? "")
        ? process.env.XDG_STATE_HOME
        : path.join(os.homedir(), ".local", "state");
    const registration = path.join(stateHome, "revisar", "checkout.json");
    const installedLoader = path.join(
        agentDir,
        "extensions",
        "revisar",
        "index.ts",
    );

    // Avoid silently replacing an independently maintained extension. Rerunning
    // setup may update our managed loader, but never an arbitrary user's code.
    try {
        const current = await readFile(installedLoader, "utf8");
        if (!current.startsWith(marker)) {
            throw new Error(
                `Refusing to replace an unmanaged extension: ${installedLoader}. Move it out of Pi's extensions directory first.`,
            );
        }
    } catch (error) {
        if (error.code !== "ENOENT") throw error;
    }
    await atomicWrite(
        registration,
        `${JSON.stringify({ checkout }, null, 2)}\n`,
    );
    await atomicWrite(installedLoader, loader);
    console.log(`Registered checkout: ${checkout}`);
    console.log(`Machine-local registration: ${registration}`);
    console.log(`Pi loader: ${installedLoader}`);
    console.log(
        "Build with cargo build --release --locked, then run /reload in Pi.",
    );
}

setup().catch((error) => {
    console.error(`revisar setup: ${error.message}`);
    process.exitCode = 1;
});
