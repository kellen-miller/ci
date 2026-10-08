import {analyzeCommits as analyze} from "@semantic-release/commit-analyzer";
import {generateNotes as generate} from "@semantic-release/release-notes-generator";

import {filterCommits, parseScopePaths} from "./scope.js";

async function scopedContext(context) {
    const scopePaths = parseScopePaths(process.env.SEMANTIC_RELEASE_COMMIT_PATHS || "");
    if (scopePaths.length === 0) {
        throw new Error("Path-scoped semantic-release requires at least one commit path");
    }

    const commits = await filterCommits(context.commits, context.cwd, scopePaths);
    context.logger.log(
        "Found %d of %d commits under %s",
        commits.length,
        context.commits.length,
        scopePaths.join(", ")
    );

    return {...context, commits};
}

export async function analyzeCommits({analyzer = {}}, context) {
    return analyze(analyzer, await scopedContext(context));
}

export async function generateNotes({notes = {}}, context) {
    return generate(notes, await scopedContext(context));
}
