#!/usr/bin/env node
import { mkdir, readdir, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { piPackage } from "./pi-package.mjs";

const checkout = fileURLToPath(new URL("../", import.meta.url));
function run(command, args) {
    const result = spawnSync(command, args, {
        cwd: checkout,
        stdio: "inherit",
    });
    if (result.error) throw result.error;
    if (result.status !== 0)
        throw new Error(
            `${command} failed (${result.signal ?? result.status})`,
        );
}

try {
    const pi = piPackage();
    // Machine-dependent type paths are generated under target/, never tracked.
    const config = path.join(checkout, "target", "pi-check", "tsconfig.json");
    await mkdir(path.dirname(config), { recursive: true });
    await writeFile(
        config,
        JSON.stringify(
            {
                compilerOptions: {
                    target: "ES2022",
                    module: "ESNext",
                    moduleResolution: "Bundler",
                    strict: true,
                    noEmit: true,
                    skipLibCheck: true,
                    paths: {
                        "@earendil-works/pi-coding-agent": [
                            path.join(pi, "dist", "index.d.ts"),
                        ],
                    },
                    typeRoots: [path.join(pi, "node_modules", "@types")],
                    types: ["node"],
                },
                include: [path.join(checkout, "integrations", "pi", "*.ts")],
            },
            null,
            2,
        ),
    );
    run("tsgo", ["-p", config]);
    run("prettier", [
        "--check",
        "integrations/pi",
        "scripts",
        ".prettierrc.json",
    ]);
    const tests = (await readdir(path.join(checkout, "integrations", "pi")))
        .filter((name) => name.endsWith(".test.mjs"))
        .sort()
        .map((name) => path.join("integrations", "pi", name));
    run(process.execPath, [
        "--experimental-test-module-mocks",
        "--test",
        ...tests,
    ]);
} catch (error) {
    console.error(`revisar Pi checks: ${error.message}`);
    process.exitCode = 1;
}
