import assert from "node:assert/strict";
import {execFileSync} from "node:child_process";
import {mkdirSync, mkdtempSync, writeFileSync} from "node:fs";
import {tmpdir} from "node:os";
import {join} from "node:path";
import test from "node:test";

import {analyzeCommits, generateNotes} from "./index.js";
import {filesMatchScope, filterCommits, parseScopePaths} from "./scope.js";

test("parses repository-relative scope paths", () => {
    assert.deepEqual(parseScopePaths("./cmd/service/,\npkg/shared"), ["cmd/service", "pkg/shared"]);
    assert.deepEqual(parseScopePaths("./"), ["."]);
    assert.throws(() => parseScopePaths("../service"), /repository-relative/);
    assert.throws(() => parseScopePaths("/service"), /repository-relative/);
});

test("matches complete path segments", () => {
    assert.equal(filesMatchScope(["cmd/service/main.go"], ["cmd/service"]), true);
    assert.equal(filesMatchScope(["cmd/service-other/main.go"], ["cmd/service"]), false);
    assert.equal(filesMatchScope(["go.work"], ["go.work"]), true);
});

test("ignores an unrelated breaking commit during analysis", async () => {
    const repository = mkdtempSync(join(tmpdir(), "semantic-release-path-scope-"));
    git(repository, "init");
    git(repository, "config", "user.name", "CI Test");
    git(repository, "config", "commit.gpgsign", "false");
    git(repository, "config", "user.email", "ci@example.com");

    const authzHash = commit(repository, "cmd/authz/main.go", "authz", "feat(authz)!: replace policies");
    const serviceHash = commit(repository, "cmd/service/main.go", "service", "fix(service): skip empty proxy");
    const serviceDirectory = join(repository, "cmd/service");

    process.env.SEMANTIC_RELEASE_COMMIT_PATHS = "cmd/service";
    const context = {
        commits: [
            {hash: serviceHash, message: "fix(service): skip empty proxy"},
            {hash: authzHash, message: "feat(authz)!: replace policies\n\nBREAKING CHANGE: replace policies"},
        ],
        cwd: serviceDirectory,
        logger: {log() {}},
    };
    const release = await analyzeCommits(
        {analyzer: {releaseRules: [{type: "refactor", release: "patch"}]}},
        context
    );

    assert.equal(release, "patch");

    const notes = await generateNotes({}, {
        ...context,
        lastRelease: {gitHead: authzHash, gitTag: "cmd/service/v1.3.0"},
        nextRelease: {
            gitHead: serviceHash,
            gitTag: "cmd/service/v1.3.1",
            version: "1.3.1",
        },
        options: {repositoryUrl: "https://github.com/example/project.git"},
    });

    assert.match(notes, /skip empty proxy/);
    assert.doesNotMatch(notes, /replace policies/);
});

test("does not attribute merge commit messages to module paths", async () => {
    const repository = mkdtempSync(join(tmpdir(), "semantic-release-path-scope-merge-"));
    git(repository, "init", "-b", "main");
    git(repository, "config", "user.name", "CI Test");
    git(repository, "config", "commit.gpgsign", "false");
    git(repository, "config", "user.email", "ci@example.com");

    commit(repository, "README.md", "initial", "chore: initialize repository");
    git(repository, "checkout", "-b", "authz");
    commit(repository, "cmd/authz/main.go", "authz", "feat(authz): replace policies");
    git(repository, "checkout", "main");
    commit(repository, "cmd/service/main.go", "service", "fix(service): skip empty proxy");
    git(repository, "merge", "--no-ff", "authz", "-m", "feat(authz)!: merge policy replacement");

    const mergeHash = git(repository, "rev-parse", "HEAD");
    const mergeCommit = {hash: mergeHash, message: "feat(authz)!: merge policy replacement"};
    const filtered = await filterCommits([mergeCommit], repository, ["cmd/service"]);

    assert.deepEqual(filtered, []);
});

test("attributes cross-module moves to the source module", async () => {
    const repository = mkdtempSync(join(tmpdir(), "semantic-release-path-scope-move-"));
    git(repository, "init", "-b", "main");
    git(repository, "config", "user.name", "CI Test");
    git(repository, "config", "commit.gpgsign", "false");
    git(repository, "config", "user.email", "ci@example.com");

    commit(repository, "cmd/service/main.go", "service", "feat(service): add service");
    mkdirSync(join(repository, "cmd/other"), {recursive: true});
    git(repository, "mv", "cmd/service/main.go", "cmd/other/main.go");
    git(repository, "commit", "-m", "refactor: move service command");

    const moveHash = git(repository, "rev-parse", "HEAD");
    const moveCommit = {hash: moveHash, message: "refactor: move service command"};
    const filtered = await filterCommits([moveCommit], repository, ["cmd/service"]);

    assert.deepEqual(filtered, [moveCommit]);
});

function git(cwd, ...args) {
    return execFileSync("git", args, {cwd, encoding: "utf8"}).trim();
}

function commit(repository, file, contents, message) {
    const path = join(repository, file);
    mkdirSync(path.slice(0, path.lastIndexOf("/")), {recursive: true});
    writeFileSync(path, contents);
    git(repository, "add", file);
    git(repository, "commit", "-m", message);
    return git(repository, "rev-parse", "HEAD");
}
