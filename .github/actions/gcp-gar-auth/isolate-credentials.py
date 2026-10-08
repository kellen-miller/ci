#!/usr/bin/env python3
"""Keep generated cloud credentials out of Docker build contexts and artifacts."""

import os
import shutil
import tempfile
from pathlib import Path

source = Path(os.environ["CREDENTIALS_FILE"])
directory = Path(tempfile.mkdtemp(prefix="cloud-credentials-", dir=os.environ["RUNNER_TEMP"]))
destination = directory / "credentials.json"
shutil.move(source, destination)
destination.chmod(0o600)
with Path(os.environ["GITHUB_ENV"]).open("a") as output:
    for name in (
        "GOOGLE_APPLICATION_CREDENTIALS",
        "CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE",
        "GOOGLE_GHA_CREDS_PATH",
    ):
        output.write(f"{name}={destination}\n")

with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
    output.write(f"path={destination}\n")
