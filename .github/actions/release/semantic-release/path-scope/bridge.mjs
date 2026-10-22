// Required adapter: semantic-release invokes JavaScript plugin callbacks.
import {execFileSync} from "node:child_process";
import {analyzeCommits as analyze} from "@semantic-release/commit-analyzer";
import {generateNotes as generate} from "@semantic-release/release-notes-generator";

function scoped(context) {
  const hashes = new Set(JSON.parse(execFileSync(process.env.CI_RELEASE_BINARY, ["release-filter"], {
    input: JSON.stringify({hashes: context.commits.map(commit => commit.hash), cwd: context.cwd, paths: JSON.parse(process.env.CI_RELEASE_PATHS)}),
    encoding: "utf8", maxBuffer: Infinity,
  })));
  const commits = context.commits.filter(commit => hashes.has(commit.hash));
  return {...context, commits};
}

export async function analyzeCommits({analyzer}, context) {
  return analyze(analyzer, scoped(context));
}

export async function generateNotes({notes}, context) {
  return generate(notes, scoped(context));
}
