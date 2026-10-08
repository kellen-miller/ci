import hashlib
import io
import os
import platform
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path

SCRIPT = (
    Path(__file__).resolve().parents[1] / ".github/actions/helm-lint-charts/install-kubeconform.sh"
)


class KubeconformInstallTests(unittest.TestCase):
    def test_checksum_is_required_before_installing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            architecture = "arm64" if platform.machine() in {"arm64", "aarch64"} else "amd64"
            archive = root / f"kubeconform-{platform.system().lower()}-{architecture}.tar.gz"
            binary = b"#!/bin/sh\necho verified-fixture\n"
            with tarfile.open(archive, "w:gz") as output:
                entry = tarfile.TarInfo("kubeconform")
                entry.size = len(binary)
                entry.mode = 0o755
                output.addfile(entry, io.BytesIO(binary))

            curl = root / "curl"
            curl.write_text(
                "#!/usr/bin/env bash\nset -euo pipefail\n"
                'for arg in "$@"; do\n'
                '  if [[ "$arg" == https://* ]]; then source="${arg##*/}"; fi\n'
                "done\n"
                'cp "$ASSETS/$source" "${@: -1}"\n'
            )
            curl.chmod(0o755)
            env = dict(
                os.environ,
                PATH=f"{root}:{os.environ['PATH']}",
                ASSETS=directory,
                KUBECONFORM_VERSION="v0.8.0",
                KUBECONFORM_INSTALL_DIR=str(root / "bin"),
                GITHUB_PATH=str(root / "github-path"),
            )
            for valid in [False, True]:
                with self.subTest(valid=valid):
                    digest = hashlib.sha256(archive.read_bytes()).hexdigest() if valid else "0" * 64
                    (root / "CHECKSUMS").write_text(f"{digest}  {archive.name}\n")
                    result = subprocess.run(
                        ["bash", str(SCRIPT)],
                        env=env,
                        capture_output=True,
                        text=True,
                        check=False,
                    )
                    if valid:
                        self.assertEqual(result.returncode, 0, result.stderr)
                        self.assertIn("verified-fixture", result.stdout)
                    else:
                        self.assertNotEqual(result.returncode, 0)
                        self.assertFalse((root / "bin/kubeconform").exists())
