import {execFileSync} from "node:child_process";
import {appendFileSync, mkdtempSync, rmSync, writeFileSync} from "node:fs";
import {tmpdir} from "node:os";
import {resolve, join} from "node:path";
import {createRequire} from "node:module";
import {fileURLToPath, pathToFileURL} from "node:url";

import semanticRelease from "semantic-release";
import semver from "semver";

const cwd = resolve(process.env.WORKING_DIRECTORY || ".");
const dryRun = process.env.DRY_RUN === "true";
const firstVersion = process.env.FIRST_RELEASE_VERSION || "";
if (firstVersion && !semver.valid(firstVersion)) {
    throw new Error("first-release-version must be a valid semver version");
}

let paths = process.env.SEMANTIC_RELEASE_COMMIT_PATHS || "";
if (!paths.trim() && cwd !== resolve(process.env.GITHUB_WORKSPACE || ".")) {
    paths = execFileSync("git", ["rev-parse", "--show-prefix"], {cwd, encoding: "utf8"}).trim();
}

process.env.SEMANTIC_RELEASE_COMMIT_PATHS = paths;
const extraPlugins = (process.env.EXTRA_PLUGINS || "").split(/[\n,]+/).map(value => value.trim()).filter(Boolean);
if (extraPlugins.length) {
    if (extraPlugins.some(spec => {
        const version = spec.slice(spec.lastIndexOf("@") + 1);
        return !/^(@[a-z0-9][\w.-]*\/[a-z0-9][\w.-]*|[a-z0-9][\w.-]*)@[^\s/]+$/.test(spec)
            || (!semver.validRange(version) && !/^[a-zA-Z][\w.-]*$/.test(version));
    })) {
        throw new Error("Extra plugins must be public npm package@version specifications");
    }

    execFileSync("npm", ["install", "--prefix", fileURLToPath(new URL(".", import.meta.url)),
        "--no-save", "--package-lock=false", "--ignore-scripts", "--no-audit", "--no-fund",
        "--registry=https://registry.npmjs.org", ...extraPlugins], {stdio: "inherit"});
}

const configFile = process.env.CONFIG_FILE
    ? pathToFileURL(resolve(cwd, process.env.CONFIG_FILE)).href
    : new URL("./release.config.cjs", import.meta.url).href;
const options = (await import(configFile)).default;
const require = createRequire(import.meta.url);
const defaults = (await import("./release.config.cjs")).default;
const settings = {
    ...options,
    plugins: [...(options.plugins || defaults.plugins),
        ...extraPlugins.map(spec => require.resolve(spec.slice(0, spec.lastIndexOf("@"))))],
    branches: (process.env.SEMANTIC_RELEASE_BRANCHES || "main").split(",").map(value => value.trim()),
    tagFormat: `${process.env.SEMANTIC_RELEASE_TAG_PREFIX || "v"}\${version}`,
};
let result;
let strategy = "none";
if (firstVersion) {
    result = await semanticRelease({...settings, dryRun: true}, {cwd});
    if (result && !result.lastRelease.version) {
        result.nextRelease.version = firstVersion;
        result.nextRelease.gitTag = settings.tagFormat.replace("${version}", firstVersion);
        strategy = "initial";
        if (!dryRun) {
            const directory = mkdtempSync(join(tmpdir(), "ci-release-"));
            try {
                const notesFile = join(directory, "notes.md");
                writeFileSync(notesFile, result.nextRelease.notes);
                execFileSync("gh", ["release", "create", result.nextRelease.gitTag,
                    "--repo", process.env.GITHUB_REPOSITORY,
                    "--target", result.nextRelease.gitHead,
                    "--title", result.nextRelease.gitTag, "--notes-file", notesFile], {cwd, stdio: "inherit"});
            } finally {
                rmSync(directory, {recursive: true, force: true});
            }
        }
    } else if (result) {
        result = await semanticRelease({...settings, dryRun}, {cwd});
        strategy = "semantic";
    }
} else {
    result = await semanticRelease({...settings, dryRun}, {cwd});
    strategy = result ? "semantic" : "none";
}

const version = result?.nextRelease.version || "";
const outputs = {
    release_published: Boolean(result) && !dryRun,
    version,
    major_minor: version ? `${semver.major(version)}.${semver.minor(version)}` : "",
    git_tag: result?.nextRelease.gitTag || "",
    release_strategy: strategy,
};
for (const [key, value] of Object.entries(outputs)) {
    appendFileSync(process.env.GITHUB_OUTPUT, `${key}=${value}\n`);
}
