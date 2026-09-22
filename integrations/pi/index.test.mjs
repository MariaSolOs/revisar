// Run with: node --experimental-test-module-mocks --test index.test.mjs
// Ghostty's native API is mocked; the generated shell wrapper executes for real.
import assert from "node:assert/strict";
import { execFile as realExecFile, spawn } from "node:child_process";
import {
    access,
    mkdir,
    mkdtemp,
    readFile,
    rm,
    writeFile,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { PassThrough } from "node:stream";
import { mock, test } from "node:test";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

// Exercise the macOS launcher on either supported host without live automation.
Object.defineProperty(process, "platform", { value: "darwin" });
const checkout = fileURLToPath(new URL("../../", import.meta.url));
const binary = path.join(checkout, "target", "release", "revisar");
const realExec = promisify(realExecFile);
let scenario;
const quote = (s) => `'${s.replace(/'/g, `'\\''`)}'`;
function execFile() {}
execFile[promisify.custom] = (file, args, options) => {
    const stdin = new PassThrough();
    const chunks = [];
    stdin.on("data", (chunk) => chunks.push(chunk));
    const finished = new Promise((resolve) => stdin.on("finish", resolve));
    const pending = (async () => {
        if (file === "git") {
            assert.deepEqual(args, ["rev-parse", "--show-toplevel"]);
            scenario.gitCwd = options.cwd;
            if (scenario.gitError) throw new Error("not a git repository");
            return { stdout: `${scenario.root}\n`, stderr: "" };
        }
        if (file.endsWith("osascript")) {
            assert.equal(args[0], "-e");
            assert(!args[1].includes("System Events"));
            if (args[1].includes("close tab candidate")) {
                assert(args[1].includes("id of candidate is item 1 of argv"));
                assert(!args[1].includes("front window"));
                scenario.closed.push(args[2]);
                if (scenario.closeError) throw new Error("Close denied");
                return { stdout: "", stderr: "" };
            }
            assert(args[1].includes("set wait after command of cfg to false"));
            assert(
                args[1].includes(
                    "new tab in front window with configuration cfg",
                ),
            );
            scenario.command = args[2];
            const script = scenario.command.match(/^\/bin\/sh '(.*)'$/)[1];
            scenario.dir = path.dirname(script);
            if (scenario.launchError) throw new Error("Automation denied");
            const source = await readFile(script, "utf8");
            assert(!source.includes("tuicr"));
            assert(!source.includes("XDG_"));
            assert(!source.includes("keystroke"));
            if (scenario.onLaunch) {
                await scenario.onLaunch();
            } else {
                const fake = path.join(scenario.dir, "fake.sh");
                await writeFile(fake, scenario.program);
                const executableWrapper = source.replace(
                    quote(binary),
                    `/bin/sh ${quote(fake)}`,
                );
                await writeFile(script, executableWrapper);
                // Nonzero revisar status belongs in the marker, not Ghostty's
                // process exit status. Even cancel/error must exit cleanly.
                await realExec("/bin/sh", [script]);
                scenario.status = await readFile(
                    path.join(scenario.dir, "status"),
                    "utf8",
                );
            }
            return { stdout: "review-tab-id\n", stderr: "" };
        }
        await finished;
        throw new Error(
            `Unexpected command: ${file}: ${Buffer.concat(chunks)}`,
        );
    })();
    pending.child = { stdin };
    return pending;
};
mock.module("node:child_process", { namedExports: { execFile, spawn } });
const { default: extension } = await import("./index.ts");

async function setup(t, options = {}) {
    // An awkward repo path also exercises the wrapper's shell quoting.
    const root = await mkdtemp(
        path.join(checkout, "target", "revisar-test-'repo-"),
    );
    scenario = {
        root,
        program: "printf 'review feedback\\n'\nexit 0\n",
        closed: [],
        ...options,
    };
    const current = scenario;
    t.after(async () => {
        await rm(root, { recursive: true, force: true });
        if (current.dir)
            await rm(current.dir, { recursive: true, force: true });
    });
    let handler;
    let shutdown;
    const sent = [];
    const notifications = [];
    const editor = [];
    const statuses = [];
    const ctx = {
        mode: "tui",
        cwd: root,
        isIdle: () => true,
        hasPendingMessages: () => false,
        sessionManager: { getSessionId: () => "original-session" },
        ui: {
            notify: (message) => notifications.push(message),
            setStatus: (_, value) => statuses.push(value),
            setEditorText: (text) => editor.push(text),
        },
    };
    extension({
        registerCommand: (name, command) => {
            assert.equal(name, "revisar");
            handler = command.handler;
        },
        on: (name, fn) => {
            assert.equal(name, "session_shutdown");
            shutdown = fn;
        },
        sendUserMessage: (text, options) => {
            if (scenario.sendError) throw new Error("Handoff failed");
            sent.push({ text, options });
        },
    });
    return {
        ctx,
        sent,
        notifications,
        editor,
        statuses,
        run: (args = "") => handler(args, ctx),
        shutdown: () => shutdown({}, ctx),
    };
}

test("explicit send delivers once, then removes transport", async (t) => {
    const h = await setup(t);
    await h.run();
    assert.deepEqual(h.sent, [
        { text: "review feedback", options: { deliverAs: "followUp" } },
    ]);
    assert.equal(scenario.gitCwd, h.ctx.cwd);
    assert.equal(h.statuses.at(-1), undefined);
    assert.equal(scenario.status, "0");
    assert.deepEqual(scenario.closed, ["review-tab-id"]);
    await assert.rejects(access(scenario.dir), { code: "ENOENT" });
});

test("cancel and error never deliver even if stdout contains text", async (t) => {
    for (const code of [1, 2]) {
        const h = await setup(t, {
            program: `printf 'partial feedback'\nprintf 'failure' >&2\nexit ${code}\n`,
        });
        await h.run();
        assert.equal(scenario.status, String(code));
        assert.deepEqual(scenario.closed, ["review-tab-id"]);
        assert.equal(h.sent.length, 0);
        assert(
            h.notifications.some((n) =>
                n.includes(code === 2 ? "cancelled" : "failure"),
            ),
        );
        await assert.rejects(access(scenario.dir), { code: "ENOENT" });
    }
});

test("empty feedback does not start a turn", async (t) => {
    const h = await setup(t, { program: "exit 0\n" });
    await h.run();
    assert.equal(h.sent.length, 0);
    assert(h.notifications.some((n) => n.includes("no comments")));
});

test("failed handoff leaves feedback in the editor, not a saved review", async (t) => {
    const h = await setup(t, { sendError: true });
    await h.run();
    assert.deepEqual(h.editor, ["review feedback"]);
    assert.equal(h.sent.length, 0);
    await assert.rejects(access(scenario.dir), { code: "ENOENT" });
});

test("shutdown abandons review and concurrent launches are rejected", async (t) => {
    const h = await setup(t);
    scenario.onLaunch = async () => {
        await h.run();
        assert(h.notifications.some((n) => n.includes("already open")));
        await h.shutdown();
    };
    await h.run();
    assert.equal(h.sent.length, 0);
    assert.equal(h.editor.length, 0);
    assert.deepEqual(scenario.closed, ["review-tab-id"]);
    await assert.rejects(access(scenario.dir), { code: "ENOENT" });
});

test("tab-close failure does not suppress submitted feedback", async (t) => {
    const h = await setup(t, { closeError: true });
    await h.run();
    assert.equal(h.sent.length, 1);
    assert.deepEqual(scenario.closed, ["review-tab-id"]);
    assert(
        h.notifications.some((n) =>
            n.includes("Could not close the revisar tab"),
        ),
    );
    await assert.rejects(access(scenario.dir), { code: "ENOENT" });
});

test("failed launch never closes an unrelated tab", async (t) => {
    const h = await setup(t, { launchError: true });
    await h.run();
    assert.equal(h.sent.length, 0);
    assert.deepEqual(scenario.closed, []);
    assert(h.notifications.some((n) => n.includes("Automation denied")));
    await assert.rejects(access(scenario.dir), { code: "ENOENT" });
});

test("--repo resolves paths from Pi's cwd and runs the wrapper in the Git root", async (t) => {
    const cases = [
        ["relative", () => "--repo ../other"],
        ["equals", () => "--repo=../other"],
        ["single quotes", () => "--repo '../other repo'"],
        ["double quotes", () => '--repo="../other repo"'],
        ["absolute with apostrophe", (root) => `--repo "${root}/other's repo"`],
        [
            "home",
            (root) => `--repo "~/${path.relative(os.homedir(), root)}/other"`,
        ],
        ["home only", () => "--repo=~"],
    ];
    for (const [name, args] of cases) {
        await t.test(name, async (t) => {
            const h = await setup(t, { program: "pwd -P\n" });
            const root = scenario.root;
            h.ctx.cwd = path.join(root, "session");
            await mkdir(h.ctx.cwd);
            const target = path.join(
                root,
                name.includes("quotes")
                    ? "other repo"
                    : name.includes("apostrophe")
                      ? "other's repo"
                      : "other",
            );
            await mkdir(target);
            await h.run(args(root));
            assert.equal(
                scenario.gitCwd,
                name === "home only" ? os.homedir() : target,
            );
            // The mock reports a parent Git root, as for a repo subdirectory.
            assert.deepEqual(h.sent, [
                { text: root, options: { deliverAs: "followUp" } },
            ]);
            assert.equal(h.ctx.cwd, path.join(root, "session"));
            await assert.rejects(access(scenario.dir), { code: "ENOENT" });
        });
    }
});

test("invalid --repo arguments never inspect Git or launch", async (t) => {
    const h = await setup(t);
    for (const args of [
        "--staged",
        "other",
        "--repo",
        "--repo=",
        '--repo ""',
        "--repo --staged",
        "--repo ../other extra",
        "--repo ../other --repo ../another",
        '--repo "unterminated',
        "--repo='mismatched\"",
    ]) {
        await h.run(args);
        assert.match(
            h.notifications.at(-1),
            /Usage: \/revisar \[--repo <path>\]/,
        );
    }
    assert.equal(scenario.gitCwd, undefined);
    assert.equal(scenario.command, undefined);
    assert.equal(h.sent.length, 0);
});

test("missing paths, files, and non-repositories do not launch", async (t) => {
    const h = await setup(t);
    await writeFile(path.join(h.ctx.cwd, "file"), "not a directory");
    for (const target of ["missing", "file"]) {
        await h.run(`--repo ${target}`);
        assert.match(h.notifications.at(-1), /Invalid --repo/);
        assert.equal(scenario.gitCwd, undefined);
    }
    scenario.gitError = true;
    await h.run("--repo .");
    assert.match(h.notifications.at(-1), /not a git repository/);
    assert.equal(scenario.command, undefined);
    assert.equal(scenario.dir, undefined);
    assert.equal(h.sent.length, 0);
    // Validation failures must release the active-review guard.
    scenario.gitError = false;
    await h.run();
    assert.equal(h.sent.length, 1);
});

test("busy agent and unexpected arguments do not launch", async (t) => {
    const h = await setup(t);
    h.ctx.isIdle = () => false;
    await h.run();
    assert(h.notifications.some((n) => n.includes("Wait for the agent")));
    h.ctx.isIdle = () => true;
    await h.run("--staged");
    assert(h.notifications.some((n) => n.includes("Usage: /revisar")));
    assert.equal(scenario.command, undefined);
    assert.equal(h.sent.length, 0);
});
