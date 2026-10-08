import {execFile} from "node:child_process";
import {promisify} from "node:util";

const execFileAsync = promisify(execFile);
const commitFilesCache = new Map();

export function parseScopePaths(value) {
    return value
        .split(/[\n,]+/)
        .map((scopePath) => scopePath.trim().replaceAll("\\", "/"))
        .filter(Boolean)
        .map((scopePath) => {
            if (/^\.(?:\/+)?$/.test(scopePath)) {
                return ".";
            }

            return scopePath.replace(/^\.\/+/, "").replace(/\/+$/, "");
        })
        .map((scopePath) => {
            if (scopePath.startsWith("/") || scopePath.split("/").includes("..")) {
                throw new Error(`Release commit paths must be repository-relative: ${scopePath}`);
            }

            return scopePath;
        });
}

export function filesMatchScope(files, scopePaths) {
    return files.some((file) => {
        const normalizedFile = file.replaceAll("\\", "/");

        return scopePaths.some((scopePath) =>
            scopePath === "." || normalizedFile === scopePath || normalizedFile.startsWith(`${scopePath}/`)
        );
    });
}

async function commitFiles(cwd, hash) {
    if (!/^[0-9a-f]{7,64}$/i.test(hash)) {
        throw new Error(`Invalid commit hash from semantic-release: ${hash}`);
    }

    const cacheKey = `${cwd}:${hash}`;
    if (!commitFilesCache.has(cacheKey)) {
        commitFilesCache.set(
            cacheKey,
            execFileAsync(
                "git",
                [
                    "diff-tree",
                    "--root",
                    "--no-commit-id",
                    "--name-only",
                    "--no-renames",
                    "-r",
                    hash,
                ],
                {cwd, encoding: "utf8", maxBuffer: 10 * 1024 * 1024}
            ).then(({stdout}) => stdout.split("\n").filter(Boolean))
        );
    }

    return commitFilesCache.get(cacheKey);
}

export async function filterCommits(commits, cwd, scopePaths) {
    const scopedCommits = [];
    for (const commit of commits) {
        const files = await commitFiles(cwd, commit.hash);
        if (filesMatchScope(files, scopePaths)) {
            scopedCommits.push(commit);
        }
    }

    return scopedCommits;
}
