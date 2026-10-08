"""Select currently ready capacity without queuing on an unavailable self-hosted runner."""

import json
import os
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


def main():
    preferred = os.environ["PREFERRED_RUNNER"]
    fallback = os.environ["FALLBACK_RUNNER"]
    for label in (preferred, fallback):
        if not label.strip() or "\n" in label or "\r" in label:
            raise ValueError("Runner labels must be nonempty single-line values")

    api = os.environ.get("GITHUB_API_URL", "https://api.github.com").rstrip("/")
    repository = urllib.parse.quote(os.environ["GITHUB_REPOSITORY"], safe="/")
    selected = fallback
    self_hosted = False
    page = 1
    try:
        while True:
            request = urllib.request.Request(
                f"{api}/repos/{repository}/actions/runners?per_page=100&page={page}",
                headers={
                    "Authorization": f"Bearer {os.environ['RUNNER_STATUS_TOKEN']}",
                    "Accept": "application/vnd.github+json",
                    "X-GitHub-Api-Version": "2026-03-10",
                },
            )
            with urllib.request.urlopen(request, timeout=10) as response:
                runners = json.load(response)["runners"]

            if any(
                runner["status"] == "online"
                and not runner["busy"]
                and preferred in {label["name"] for label in runner["labels"]}
                for runner in runners
            ):
                selected = preferred
                self_hosted = True
                break

            if len(runners) < 100:
                break

            page += 1
    except urllib.error.HTTPError as error:
        print(f"::warning::Runner lookup returned HTTP {error.code}; use hosted fallback.")
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, KeyError, TypeError):
        print("::warning::Runner availability could not be confirmed; use hosted fallback.")

    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
        output.write(f"runner={selected}\nself-hosted={str(self_hosted).lower()}\n")

    print(f"Selected runner: {selected}")


if __name__ == "__main__":
    main()
