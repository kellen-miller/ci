// Required adapter: semantic-release invokes JavaScript plugin callbacks.
import {execFileSync} from "node:child_process";
import {analyzeCommits as analyze} from "@semantic-release/commit-analyzer";
import {generateNotes as generate} from "@semantic-release/release-notes-generator";

function scoped(context) {
  const commits = JSON.parse(execFileSync(process.env.CI_RELEASE_BINARY, ["release-filter"], {
    input: JSON.stringify({commits: context.commits, cwd: context.cwd, paths: JSON.parse(process.env.CI_RELEASE_PATHS)}),
    encoding: "utf8", maxBuffer: 10 * 1024 * 1024,
  }));
  return {...context, commits};
}

export async function analyzeCommits({analyzer}, context) {
  return analyze(analyzer, scoped(context));
}

export async function generateNotes({notes}, context) {
  return generate(notes, scoped(context));
}
