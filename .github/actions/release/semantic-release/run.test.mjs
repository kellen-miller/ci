import assert from "node:assert/strict";
import {execFileSync} from "node:child_process";
import {mkdtempSync, readFileSync, rmSync, writeFileSync} from "node:fs";
import {createRequire} from "node:module";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {fileURLToPath} from "node:url";
import test from "node:test";
import {generateNotes} from "@semantic-release/release-notes-generator";

const require = createRequire(import.meta.url);
const runner = fileURLToPath(new URL("./run.mjs", import.meta.url));

test("the default release preset renders notes with the installed writer", async () => {
    const config = require("./release.config.cjs");
    const [, options] = config.plugins.find(([name]) => name === require.resolve("@semantic-release/release-notes-generator"));
    const notes = await generateNotes(options, {
        cwd: process.cwd(),
        env: {},
        options: {repositoryUrl: "https://github.com/example/project"},
        commits: [{message: "feat: verify release notes", hash: "a".repeat(40)}],
        lastRelease: {},
        nextRelease: {version: "1.0.0", gitTag: "v1.0.0"},
    });

    assert.match(notes, /verify release notes/);
});

test("extra plugin inputs reject npm options and local dependencies", () => {
    for (const spec of ["--help@1", "example@file:local", "example@https://example.com/package.tgz"]) {
        assert.throws(() => execFileSync("node", [runner], {
            env: {...process.env, WORKING_DIRECTORY: ".", GITHUB_WORKSPACE: process.cwd(),
                EXTRA_PLUGINS: spec}, stdio: "pipe",
        }), /Extra plugins must be public npm/);
    }
});

test("dry runs honor initial versions and major releases without publishing", () => {
    const directory = mkdtempSync(join(tmpdir(), "ci-release-run-"));
    const repository = join(directory, "source");
    const remote = join(directory, "remote.git");
    try {
        execFileSync("git", ["init", "--bare", "--quiet", remote]);
        execFileSync("git", ["init", "--quiet", "--initial-branch=main", repository]);
        execFileSync("git", ["config", "user.name", "CI Test"], {cwd: repository});
        execFileSync("git", ["config", "user.email", "ci@example.com"], {cwd: repository});
        execFileSync("git", ["config", "commit.gpgsign", "false"], {cwd: repository});
        execFileSync("git", ["remote", "add", "origin", `file://${remote}`], {cwd: repository});
        writeFileSync(join(repository, "source.txt"), "one\n");
        execFileSync("git", ["add", "."], {cwd: repository});
        execFileSync("git", ["commit", "--quiet", "-m", "feat: add source"], {cwd: repository});
        execFileSync("git", ["push", "--quiet", "origin", "HEAD:refs/heads/main"], {cwd: repository});
        execFileSync("git", ["symbolic-ref", "HEAD", "refs/heads/main"], {cwd: remote});
        const config = join(directory, "release.cjs");
        writeFileSync(config, `module.exports = {plugins: [
            [${JSON.stringify(require.resolve("@semantic-release/commit-analyzer"))},
                {preset: "conventionalcommits"}],
            ${JSON.stringify(require.resolve("@semantic-release/release-notes-generator"))},
        ]};`);
        const output = join(directory, "output");
        const env = {...process.env, GITHUB_OUTPUT: output, GITHUB_WORKSPACE: repository,
            WORKING_DIRECTORY: repository, CONFIG_FILE: config, DRY_RUN: "true",
            CI: "true", GITHUB_ACTIONS: "true", GITHUB_REF: "refs/heads/main",
            GITHUB_EVENT_NAME: "push", GITHUB_REPOSITORY: "example/project",
            SEMANTIC_RELEASE_BRANCHES: "main", SEMANTIC_RELEASE_TAG_PREFIX: "v",
            SEMANTIC_RELEASE_COMMIT_PATHS: "", FIRST_RELEASE_VERSION: "0.1.0"};
        // Keep this test entirely local even if the developer shell contains credentials.
        delete env.GITHUB_TOKEN;
        delete env.GH_TOKEN;
        execFileSync("node", [runner], {cwd: repository, env, stdio: "pipe"});
        const initial = readFileSync(output, "utf8");
        assert.match(initial, /version=0\.1\.0\n/);
        assert.match(initial, /release_published=false/);
        assert.match(initial, /release_strategy=initial/);
        assert.equal(execFileSync("git", ["tag"], {cwd: remote, encoding: "utf8"}), "");

        execFileSync("git", ["-c", "tag.gpgsign=false", "tag", "v1.0.0"], {cwd: repository});
        execFileSync("git", ["push", "--quiet", "origin", "refs/tags/v1.0.0:refs/tags/v1.0.0"], {cwd: repository});
        writeFileSync(join(repository, "source.txt"), "two\n");
        execFileSync("git", ["add", "."], {cwd: repository});
        execFileSync("git", ["commit", "--quiet", "-m", "feat!: replace source"], {cwd: repository});
        execFileSync("git", ["push", "--quiet", "origin", "HEAD:refs/heads/main"], {cwd: repository});
        writeFileSync(output, "");
        execFileSync("node", [runner], {cwd: repository, env, stdio: "pipe"});
        assert.match(readFileSync(output, "utf8"), /version=2\.0\.0\n/);
        assert.equal(execFileSync("git", ["tag"], {cwd: remote, encoding: "utf8"}).trim(), "v1.0.0");
    } finally {
        rmSync(directory, {recursive: true, force: true});
    }
});
