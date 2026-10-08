#!/usr/bin/env python3
"""Warn before deleting unchanged stale branches in explicitly selected repositories."""

import json
import os
import re
import subprocess
from datetime import UTC, datetime, timedelta
from urllib.parse import quote


def api(endpoint, *, method="GET", body=None, paginate=False):
    command = ["gh", "api", endpoint, "--method", method]
    if paginate:
        command.extend(["--paginate", "--slurp"])

    if body is not None:
        command.extend(["--input", "-"])

    result = subprocess.run(
        command,
        input=json.dumps(body) if body is not None else None,
        text=True,
        check=True,
        capture_output=True,
    )
    value = json.loads(result.stdout) if result.stdout.strip() else None
    if paginate:
        return [item for page in value for item in page]

    return value


def main():
    repositories = list(
        dict.fromkeys(
            filter(
                None,
                re.split(
                    r"[\s,]+", os.environ.get("REPOSITORIES", "") or os.environ["GITHUB_REPOSITORY"]
                ),
            )
        )
    )
    if not all(re.fullmatch(r"[\w.-]+/[\w.-]+", repo) for repo in repositories):
        raise SystemExit("Repositories must be explicit owner/name pairs")

    stale_days = int(os.environ.get("STALE_DAYS", "30"))
    warning_days = int(os.environ.get("WARNING_DAYS", "7"))
    dry_run = os.environ.get("DRY_RUN", "true")
    if stale_days < 1 or warning_days < 1 or dry_run not in {"true", "false"}:
        raise SystemExit("Days must be positive; DRY_RUN must be true or false")

    now = datetime.now(UTC)
    marker = "<!-- personal-ci-stale-branches:"
    actor = api("graphql", method="POST", body={"query": "query { viewer { login } }"})["data"][
        "viewer"
    ]["login"]
    for repo in repositories:
        default = api(f"repos/{repo}")["default_branch"]
        issues = api(f"repos/{repo}/issues?state=open&per_page=100", paginate=True)
        reports = [
            issue
            for issue in issues
            if marker in (issue.get("body") or "")
            and "pull_request" not in issue
            and issue.get("user", {}).get("login") == actor
        ]
        if len(reports) > 1:
            raise SystemExit(f"Multiple cleanup reports in {repo}; resolve before proceeding")

        report = reports[0] if reports else None
        previous = {}
        if report:
            state = report["body"].split(marker, 1)[1].split(" -->", 1)[0]
            previous = json.loads(state)

        warnings = {}
        deleted = []
        for branch in api(f"repos/{repo}/branches?per_page=100", paginate=True):
            name = branch["name"]
            if name == default or branch["protected"]:
                continue

            encoded = quote(name, safe="")
            head = quote(repo.split("/")[0] + ":" + name, safe="")
            if api(f"repos/{repo}/pulls?state=open&head={head}"):
                continue

            commit = api(f"repos/{repo}/commits/{branch['commit']['sha']}")
            committed = datetime.fromisoformat(
                commit["commit"]["committer"]["date"].replace("Z", "+00:00")
            )
            if now - committed < timedelta(days=stale_days):
                continue

            sha = branch["commit"]["sha"]
            warned = previous.get(name, {})
            mature = (
                warned.get("sha") == sha
                and "warned_at" in warned
                and now - datetime.fromisoformat(warned["warned_at"])
                >= timedelta(days=warning_days)
            )
            print(f"{'DELETE' if mature else 'WARN'} {repo} {name} (dry-run={dry_run})")
            if mature and dry_run == "false":
                current = api(f"repos/{repo}/branches/{encoded}")
                if current["protected"] or current["commit"]["sha"] != sha:
                    continue

                if api(f"repos/{repo}/pulls?state=open&head={head}"):
                    continue

                api(f"repos/{repo}/git/refs/heads/{encoded}", method="DELETE")
                deleted.append(name)
            else:
                warnings[name] = (
                    warned
                    if warned.get("sha") == sha
                    else {"sha": sha, "warned_at": now.isoformat()}
                )

        if dry_run == "true":
            continue

        body = "Stale unprotected branches without open PRs. Push a commit to keep a branch.\n\n"
        body += f"Branches are eligible for deletion after {warning_days} days of warning.\n\n"
        body += "\n".join(f"- `{name}`" for name in warnings)
        if deleted:
            body += "\n\nDeleted:\n" + "\n".join(f"- `{name}`" for name in deleted)

        body += f"\n\n{marker}{json.dumps(warnings, sort_keys=True)} -->"
        if report:
            api(
                f"repos/{repo}/issues/{report['number']}",
                method="PATCH",
                body={"body": body, "state": "open" if warnings else "closed"},
            )
        elif warnings:
            api(
                f"repos/{repo}/issues",
                method="POST",
                body={"title": "Stale branch cleanup", "body": body},
            )


if __name__ == "__main__":
    main()
