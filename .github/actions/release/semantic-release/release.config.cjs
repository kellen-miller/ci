const path = require("node:path");

const commitPaths = process.env.SEMANTIC_RELEASE_COMMIT_PATHS || "";
const analyzer = {
    preset: "conventionalcommits",
    releaseRules: [
        {breaking: true, release: process.env.PRELAUNCH === "true" ? "minor" : "major"},
        {type: "refactor", release: "patch"},
    ],
};
const notes = {preset: "conventionalcommits"};
const commitPlugins = commitPaths.trim()
    ? [[path.join(__dirname, "path-scope/index.js"), {analyzer, notes}]]
    : [
        [require.resolve("@semantic-release/commit-analyzer"), analyzer],
        [require.resolve("@semantic-release/release-notes-generator"), notes],
    ];

module.exports = {
    plugins: [
        ...commitPlugins,
        [require.resolve("@semantic-release/github"), {
            successComment: false, failComment: false, releasedLabels: false,
        }],
    ],
};
