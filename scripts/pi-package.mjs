// Development checks use the installed Pi's types and loader, not a second
// npm copy or a hardcoded Node-version-manager path.
import { execFileSync } from "node:child_process";
import { accessSync } from "node:fs";
import path from "node:path";

export function piPackage() {
    const dir =
        process.env.PI_PACKAGE_DIR ||
        path.join(
            execFileSync("npm", ["root", "-g"], { encoding: "utf8" }).trim(),
            "@earendil-works",
            "pi-coding-agent",
        );
    accessSync(path.join(dir, "dist", "index.d.ts"));
    accessSync(path.join(dir, "dist", "core", "extensions", "loader.js"));
    return dir;
}
