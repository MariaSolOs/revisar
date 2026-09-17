// Real Pi loader tests: registration, dynamic imports, and /reload semantics.
// All fake homes, checkouts, and state files remain under this repo's target/.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import {
    copyFile,
    mkdir,
    mkdtemp,
    readFile,
    realpath,
    rename,
    rm,
    stat,
    writeFile,
} from "node:fs/promises";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { promisify } from "node:util";
import { piPackage } from "../../scripts/pi-package.mjs";

const exec = promisify(execFile);
const source = fileURLToPath(new URL("../../", import.meta.url));
const { loadExtensions, clearExtensionCache } = await import(
    pathToFileURL(
        path.join(piPackage(), "dist", "core", "extensions", "loader.js"),
    ).href
);

async function fixture(t, { xdg = true, customAgentDir = true } = {}) {
    await mkdir(path.join(source, "target"), { recursive: true });
    const dir = await mkdtemp(path.join(source, "target", "pi-setup-"));
    const home = path.join(dir, "home");
    const checkout = path.join(dir, "checkout '界 #percent%\nspace");
    const stateHome = xdg
        ? path.join(dir, "state")
        : path.join(home, ".local", "state");
    const agentDir = customAgentDir
        ? path.join(dir, "pi profile")
        : path.join(home, ".pi", "agent");
    await mkdir(home, { recursive: true });
    await mkdir(path.join(checkout, "scripts"), { recursive: true });
    await mkdir(path.join(checkout, "integrations", "pi"), { recursive: true });
    for (const file of ["scripts/setup-pi.mjs", "integrations/pi/loader.ts"]) {
        await copyFile(path.join(source, file), path.join(checkout, file));
    }
    const env = {
        ...process.env,
        HOME: home,
        XDG_STATE_HOME: xdg ? stateHome : "relative-is-ignored",
    };
    if (customAgentDir) env.PI_CODING_AGENT_DIR = agentDir;
    else delete env.PI_CODING_AGENT_DIR;
    const previous = Object.fromEntries(
        ["HOME", "XDG_STATE_HOME"].map((key) => [key, process.env[key]]),
    );
    process.env.HOME = home;
    process.env.XDG_STATE_HOME = env.XDG_STATE_HOME;
    t.after(async () => {
        for (const [key, value] of Object.entries(previous)) {
            if (value === undefined) delete process.env[key];
            else process.env[key] = value;
        }
        clearExtensionCache();
        await rm(dir, { recursive: true, force: true });
    });
    const f = {
        dir,
        home,
        checkout,
        env,
        agentDir,
        loader: path.join(agentDir, "extensions", "revisar", "index.ts"),
        registration: path.join(stateHome, "revisar", "checkout.json"),
    };
    await implementation(f, "v1");
    return f;
}

async function implementation(f, version) {
    await writeFile(
        path.join(f.checkout, "integrations", "pi", "index.ts"),
        `
export default function (pi) {
    pi.registerCommand("revisar", { description: ${JSON.stringify(version)}, handler: async () => {} });
}
`,
    );
}

async function setup(f) {
    return exec(
        process.execPath,
        [path.join(f.checkout, "scripts", "setup-pi.mjs")],
        {
            cwd: f.home,
            env: f.env,
        },
    );
}

async function load(f) {
    // This is Pi's actual extension module loader, not native import() with a
    // test-only cache-busting query. Each call represents a /reload.
    clearExtensionCache();
    const result = await loadExtensions([f.loader], f.home);
    assert.deepEqual(result.errors, []);
    assert.equal(result.extensions.length, 1);
    assert.equal(result.extensions[0].commands.size, 1);
    return result.extensions[0].commands.get("revisar");
}

async function assertSetupMessage(f) {
    const command = await load(f);
    const messages = [];
    await command.handler("", {
        ui: { notify: (text) => messages.push(text) },
    });
    assert.equal(messages.length, 1);
    assert(messages[0].includes("node scripts/setup-pi.mjs"));
    return messages[0];
}

test("setup is cwd-independent, private, idempotent, and path-independent", async (t) => {
    const f = await fixture(t);
    await setup(f);
    const expected = { checkout: await realpath(f.checkout) };
    assert.deepEqual(
        JSON.parse(await readFile(f.registration, "utf8")),
        expected,
    );
    const template = await readFile(
        path.join(source, "integrations", "pi", "loader.ts"),
        "utf8",
    );
    assert.equal(await readFile(f.loader, "utf8"), template);
    assert(!template.includes(f.checkout));
    assert.equal((await stat(f.registration)).mode & 0o077, 0);
    assert.equal((await load(f)).description, "v1");
    await setup(f);
    assert.deepEqual(
        JSON.parse(await readFile(f.registration, "utf8")),
        expected,
    );
    assert.equal((await load(f)).description, "v1");
});

test("/reload picks up implementation edits without rerunning setup", async (t) => {
    const f = await fixture(t);
    await setup(f);
    assert.equal((await load(f)).description, "v1");
    await implementation(f, "v2");
    assert.equal((await load(f)).description, "v2");
});

test("moving a checkout needs only setup and /reload", async (t) => {
    const f = await fixture(t);
    await setup(f);
    assert.equal((await load(f)).description, "v1");
    const originalLoader = await readFile(f.loader, "utf8");
    await rename(f.checkout, `${f.checkout} moved`);
    f.checkout += " moved";
    await assertSetupMessage(f);
    await implementation(f, "moved");
    await setup(f);
    assert.equal((await load(f)).description, "moved");
    assert.equal(await readFile(f.loader, "utf8"), originalLoader);
});

test("default Pi and state directories work with a relative XDG value ignored", async (t) => {
    const f = await fixture(t, { xdg: false, customAgentDir: false });
    await setup(f);
    assert.equal((await load(f)).description, "v1");
    assert.deepEqual(JSON.parse(await readFile(f.registration, "utf8")), {
        checkout: await realpath(f.checkout),
    });
});

test("synced loader gives actionable help before registration and for invalid state", async (t) => {
    const f = await fixture(t);
    await mkdir(path.dirname(f.loader), { recursive: true });
    await copyFile(
        path.join(source, "integrations", "pi", "loader.ts"),
        f.loader,
    );
    await assertSetupMessage(f);
    await setup(f);
    for (const contents of [
        "{invalid",
        "null",
        "{}",
        '{"checkout":"relative/path"}',
    ]) {
        await writeFile(f.registration, contents);
        await assertSetupMessage(f);
    }
});

test("setup never replaces an unmanaged extension or changes existing registration", async (t) => {
    const f = await fixture(t);
    await setup(f);
    const registration = await readFile(f.registration, "utf8");
    const custom = "// Hand-maintained extension\n";
    await writeFile(f.loader, custom);
    await assert.rejects(
        setup(f),
        /Refusing to replace an unmanaged extension/,
    );
    assert.equal(await readFile(f.loader, "utf8"), custom);
    assert.equal(await readFile(f.registration, "utf8"), registration);
});

test("the real implementation resolves its binary from the registered checkout", async (t) => {
    const f = await fixture(t);
    await copyFile(
        path.join(source, "integrations", "pi", "index.ts"),
        path.join(f.checkout, "integrations", "pi", "index.ts"),
    );
    await setup(f);
    const command = await load(f);
    const messages = [];
    await command.handler("", {
        mode: "tui",
        cwd: f.home,
        isIdle: () => true,
        hasPendingMessages: () => false,
        sessionManager: { getSessionId: () => "test" },
        ui: { notify: (text) => messages.push(text), setStatus: () => {} },
    });
    assert.equal(messages.length, 1);
    assert(messages[0].includes("cargo build --release --locked"));
    const registered = JSON.parse(
        await readFile(f.registration, "utf8"),
    ).checkout;
    const quoted = `'${`${registered}/`.replace(/'/g, `'\\''`)}'`;
    assert(messages[0].includes(`cd ${quoted} && cargo build`), messages[0]);
});
