import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / ".github/actions/code-duplication/check.sh"


class CodeDuplicationTests(unittest.TestCase):
    def test_report_does_not_hide_original_failure_or_change_config(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "cpd"
            binary.write_text(
                '#!/usr/bin/env bash\nprintf "%s\\n" "$*" >> "$CALL_LOG"\n'
                '[[ "$*" == *"--reporters ai"* ]] && exit 0\nexit 7\n'
            )
            binary.chmod(0o755)
            config = root / "custom config.json"
            config.write_text('{"threshold":5}')
            log = root / "calls"
            result = subprocess.run(
                ["bash", str(SCRIPT)],
                env=dict(
                    os.environ,
                    PATH=f"{root}:{os.environ['PATH']}",
                    CPD_CONFIG=str(config),
                    CALL_LOG=str(log),
                ),
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 7)
            self.assertEqual(len(log.read_text().splitlines()), 2)
            self.assertIn(str(config), log.read_text())
            self.assertEqual(config.read_text(), '{"threshold":5}')
