import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / ".github/actions/changed-paths/detect.py"
SPEC = importlib.util.spec_from_file_location("changed_paths", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ChangedPathsTests(unittest.TestCase):
    def test_filter_selection(self):
        filters = {"python": ["**/*.py"], "node": ["web/**", "package.json"]}
        for paths, expected in [
            (["README.md"], {"python": False, "node": False}),
            (["main.py"], {"python": True, "node": False}),
            (["a/b/test.py"], {"python": True, "node": False}),
            (["web/src/view.ts"], {"python": False, "node": True}),
            (["package.json"], {"python": False, "node": True}),
            (["a/test.py", "web/view.ts"], {"python": True, "node": True}),
            ([], {"python": False, "node": False}),
        ]:
            with self.subTest(paths=paths):
                self.assertEqual(MODULE.select_groups(filters, paths), expected)

    def test_git_diff_includes_deletions_renames_and_unusual_names(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.git(root, "init", "-q")
            self.git(root, "config", "user.name", "Test")
            self.git(root, "config", "user.email", "test@example.com")
            for name in ["deleted.py", "renamed.py", "space and\nnewline.py"]:
                (root / name).write_text("base")

            self.git(root, "add", ".")
            self.git(root, "commit", "-qm", "base")
            base = self.git(root, "rev-parse", "HEAD")
            (root / "deleted.py").unlink()
            (root / "renamed.py").rename(root / "new.txt")
            (root / "space and\nnewline.py").write_text("changed")
            self.git(root, "add", "-A")
            self.git(root, "commit", "-qm", "change")
            head = self.git(root, "rev-parse", "HEAD")
            result = self.detect(
                root,
                {
                    "deleted": ["deleted.py"],
                    "old_name": ["renamed.py"],
                    "new_name": ["new.txt"],
                    "unusual": ["space and\nnewline.py"],
                },
                base,
                head,
            )
            self.assertEqual(result, dict.fromkeys(result, True))
            # Changes only on base, after the PR branch point, do not select groups.
            self.assertEqual(
                self.detect(root, {"base_only": ["new.txt"]}, head, base), {"base_only": False}
            )

    def test_non_pr_selects_all_groups_without_git_history(self):
        for event in ["push", "workflow_dispatch", "schedule", "merge_group"]:
            with self.subTest(event=event), tempfile.TemporaryDirectory() as directory:
                self.assertEqual(
                    self.detect(Path(directory), {"code": ["src/**"]}, event=event), {"code": True}
                )

    def test_invalid_filters_and_missing_history_fail(self):
        for filters, base, head in [
            ({}, "", ""),
            ({"code": []}, "", ""),
            ({"code": ["!docs/**"]}, "", ""),
            ({"code": ["src/**"]}, "bad", "bad"),
        ]:
            with (
                self.subTest(filters=filters),
                tempfile.TemporaryDirectory() as directory,
                self.assertRaises(subprocess.CalledProcessError),
            ):
                self.detect(Path(directory), filters, base, head)

    def git(self, root, *args):
        return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()

    def detect(self, root, filters, base="", head="", event="pull_request"):
        output = root / "outputs"
        output.unlink(missing_ok=True)
        subprocess.run(
            ["python3", str(SCRIPT)],
            cwd=root,
            check=True,
            capture_output=True,
            env={
                **os.environ,
                "FILTERS": json.dumps(filters),
                "EVENT_NAME": event,
                "BASE_SHA": base,
                "HEAD_SHA": head,
                "GITHUB_OUTPUT": str(output),
            },
        )
        return json.loads(output.read_text().removeprefix("changes="))
