import assert from "node:assert/strict";
import {execFileSync} from "node:child_process";
import {mkdtempSync, rmSync, writeFileSync} from "node:fs";
import {createRequire} from "node:module";
import {tmpdir} from "node:os";
import {join} from "node:path";
import test from "node:test";

import {analyzeCommits} from "@semantic-release/commit-analyzer";

import {analyzeCommits as analyzeScopedCommits} from "./index.js";

const require = createRequire(import.meta.url);
const configPath = require.resolve("../release.config.cjs");

test("standard release policy applies to full and scoped histories", async () => {
    const repository = mkdtempSync(join(tmpdir(), "release-policy-"));
    const previousPaths = process.env.SEMANTIC_RELEASE_COMMIT_PATHS;

    try {
        execFileSync("git", ["init", "-q"], {cwd: repository});
        writeFileSync(join(repository, "source.txt"), "release policy fixture\n");
        execFileSync("git", ["add", "source.txt"], {cwd: repository});
        execFileSync("git", [
            "-c", "user.name=Release Test", "-c", "user.email=release@example.com",
            "-c", "commit.gpgsign=false", "commit", "-qm", "chore: initialize",
        ], {cwd: repository});
        const hash = execFileSync("git", ["rev-parse", "HEAD"], {
            cwd: repository, encoding: "utf8",
        }).trim();
        const cases = [
            ["feat: add a feature", "minor"],
            ["fix: repair a bug", "patch"],
            ["refactor: simplify a function", "patch"],
            ["schema: update a contract", null],
            ["docs: clarify usage", null],
            ["feat!: replace an API", "major"],
            ["fix(api)!: reject old input", "major"],
            ["refactor!: replace an API", "major"],
            ["feat!: replace an API\n\nBREAKING CHANGE: remove the old API", "major"],
            ["fix: reject old input\n\nBREAKING CHANGE: reject old input", "major"],
            ["refactor: replace an API\n\nBREAKING CHANGE: remove the old API", "major"],
            ["chore: replace an API\n\nBREAKING CHANGE: remove the old API", "major"],
        ];

        for (const scoped of [false, true]) {
            process.env.SEMANTIC_RELEASE_COMMIT_PATHS = scoped ? "source.txt" : "";
            delete require.cache[configPath];
            const config = require(configPath);
            const [plugin, options] = config.plugins[0];
            const analyze = scoped ? analyzeScopedCommits : analyzeCommits;
            assert.equal(plugin, scoped
                ? require.resolve("./index.js")
                : require.resolve("@semantic-release/commit-analyzer"));

            for (const [message, expected] of cases) {
                const actual = await analyze(options, {
                    cwd: repository, commits: [{hash, message}], logger: {log() {}},
                });
                assert.equal(actual, expected, `${scoped ? "scoped" : "full"}: ${message}`);
            }
        }
    } finally {
        if (previousPaths === undefined) {
            delete process.env.SEMANTIC_RELEASE_COMMIT_PATHS;
        } else {
            process.env.SEMANTIC_RELEASE_COMMIT_PATHS = previousPaths;
        }

        delete require.cache[configPath];
        rmSync(repository, {recursive: true, force: true});
    }
});
