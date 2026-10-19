import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
ACTION = ROOT / ".github/actions/rust-check"
SETUP = "actions-rust-lang/setup-rust-toolchain@ecabd13d1c56bd1345c230e542e9144811ad706f"


class RustCheckTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.project = self.root / "workspace with spaces"
        self.project.mkdir()
        self.log = self.root / "cargo.jsonl"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        cargo = self.bin / "cargo"
        cargo.write_text("""#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

if sys.argv[1] == "metadata":
    print(os.environ["CARGO_METADATA"])

with Path(os.environ["CARGO_LOG"]).open("a") as log:
    log.write(json.dumps({"argv": sys.argv[1:], "cwd": os.getcwd()}) + "\\n")

if sys.argv[1] == os.environ.get("FAIL_CHECK"):
    if not os.environ.get("FAIL_ARG") or os.environ["FAIL_ARG"] in sys.argv[2:]:
        sys.exit(int(os.environ.get("FAIL_CODE", "37")))
""")
        cargo.chmod(0o755)

    def run_check(self, **inputs):
        self.log.unlink(missing_ok=True)
        result = subprocess.run(
            ["bash", str(ACTION / "check.sh")],
            cwd=self.root,
            env={
                **os.environ,
                "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
                "CARGO_LOG": str(self.log),
                "CARGO_METADATA": json.dumps(
                    {
                        "workspace_members": ["fixture"],
                        "packages": [{"id": "fixture", "targets": [{"doctest": True}]}],
                    }
                ),
                "SETUP_TOOLCHAIN": "true",
                "WORKING_DIRECTORY": self.project.name,
                "CHECKS": "fmt clippy test build",
                "ALL_FEATURES": "true",
                "BUILD_PROFILE": "dev",
                "FAIL_CHECK": "",
                "FAIL_ARG": "",
                **inputs,
            },
            capture_output=True,
            text=True,
            check=False,
        )
        calls = (
            [json.loads(line) for line in self.log.read_text().splitlines()]
            if self.log.exists()
            else []
        )
        return result, calls

    def test_default_checks_use_exact_locked_workspace_arguments_and_cwd(self):
        result, calls = self.run_check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(
            [call["argv"] for call in calls],
            [
                ["fmt", "--all", "--", "--check"],
                [
                    "clippy",
                    "--locked",
                    "--workspace",
                    "--all-features",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ],
                ["test", "--locked", "--workspace", "--all-features", "--all-targets"],
                ["metadata", "--locked", "--no-deps", "--format-version", "1", "--all-features"],
                ["test", "--locked", "--workspace", "--all-features", "--doc"],
                [
                    "build",
                    "--locked",
                    "--workspace",
                    "--all-features",
                    "--all-targets",
                    "--profile",
                    "dev",
                ],
            ],
        )
        self.assertEqual([call["cwd"] for call in calls], [str(self.project.resolve())] * 6)

    def test_selected_checks_preserve_order_release_profile_and_default_features(self):
        result, calls = self.run_check(
            CHECKS=" build   test clippy ",
            ALL_FEATURES="false",
            BUILD_PROFILE="release",
            SETUP_TOOLCHAIN="false",
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(
            [call["argv"] for call in calls],
            [
                ["build", "--locked", "--workspace", "--all-targets", "--profile", "release"],
                ["test", "--locked", "--workspace", "--all-targets"],
                ["metadata", "--locked", "--no-deps", "--format-version", "1"],
                ["test", "--locked", "--workspace", "--doc"],
                ["clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings"],
            ],
        )

    def test_every_check_can_run_independently(self):
        for check, expected in [
            ("fmt", ["fmt"]),
            ("clippy", ["clippy"]),
            ("test", ["test", "metadata", "test"]),
            ("build", ["build"]),
        ]:
            with self.subTest(check=check):
                result, calls = self.run_check(CHECKS=check)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual([call["argv"][0] for call in calls], expected)

    def test_invalid_inputs_fail_before_any_cargo_command(self):
        for name, values in {
            "CHECKS": [
                "",
                "   ",
                "fmt unknown",
                "fmt fmt",
                "fmt clippy fmt",
                "fmt\nbuild",
                "fmt\rbuild",
                "fmt\tbuild",
                "fmt\n",
                "fmt;touch pwned",
                "fmt $(touch pwned)",
                "fmt `touch pwned`",
                "fmt *",
                "fmt --release",
            ],
            "ALL_FEATURES": ["", "True", "1", "false ", "true\n", "$(touch pwned)"],
            "SETUP_TOOLCHAIN": ["", "True", "1", "false ", "true\n", "$(touch pwned)"],
            "BUILD_PROFILE": ["", "debug", "custom", "dev release", "dev\r", ";touch pwned"],
            "WORKING_DIRECTORY": ["workspace\nname", "workspace\rname", "", "missing"],
        }.items():
            for value in values:
                with self.subTest(name=name, value=value):
                    result, calls = self.run_check(**{name: value})
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertEqual(calls, [])
                    self.assertFalse((self.root / "pwned").exists())

    def test_shell_syntax_in_working_directory_is_literal_data(self):
        directory = self.root / "$(touch pwned); `touch pwned` *"
        directory.mkdir()
        result, calls = self.run_check(WORKING_DIRECTORY=directory.name, CHECKS="fmt")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(
            calls, [{"argv": ["fmt", "--all", "--", "--check"], "cwd": str(directory.resolve())}]
        )
        self.assertFalse((self.root / "pwned").exists())

    def test_failures_preserve_exit_code_and_stop_all_later_commands(self):
        for check, argument, count in [
            ("fmt", "", 1),
            ("clippy", "", 2),
            ("test", "--all-targets", 3),
            ("metadata", "", 4),
            ("test", "--doc", 5),
            ("build", "", 6),
        ]:
            with self.subTest(check=check, argument=argument):
                result, calls = self.run_check(FAIL_CHECK=check, FAIL_ARG=argument, FAIL_CODE="43")
                self.assertEqual(result.returncode, 43, result.stdout + result.stderr)
                self.assertEqual(len(calls), count)
                self.assertEqual(calls[-1]["argv"][0], check)

    def test_doctests_use_only_workspace_members_and_metadata_errors_fail(self):
        metadata = {
            "workspace_members": ["binary"],
            "packages": [
                {"id": "binary", "targets": [{"doctest": False}]},
                {"id": "dependency", "targets": [{"doctest": True}]},
            ],
        }
        result, calls = self.run_check(CHECKS="test build", CARGO_METADATA=json.dumps(metadata))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual([call["argv"][0] for call in calls], ["test", "metadata", "build"])
        result, calls = self.run_check(CHECKS="test build", CARGO_METADATA="invalid JSON")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual([call["argv"][0] for call in calls], ["test", "metadata"])

    def test_action_contract_and_ci_install_real_components(self):
        action = yaml.safe_load((ACTION / "action.yaml").read_text())
        self.assertEqual(
            {name: value["default"] for name, value in action["inputs"].items()},
            {
                "setup-toolchain": "true",
                "working-directory": ".",
                "toolchain": "",
                "checks": "fmt clippy test build",
                "all-features": "true",
                "build-profile": "dev",
                "cache-key": "",
            },
        )
        self.assertEqual(action["runs"]["using"], "composite")
        setup, run = action["runs"]["steps"]
        self.assertEqual(setup["uses"], SETUP)
        self.assertEqual(setup["if"], "${{ inputs.setup-toolchain == 'true' }}")
        self.assertEqual(
            setup["with"],
            {
                "toolchain": "${{ inputs.toolchain }}",
                "rust-src-dir": "${{ inputs.working-directory }}",
                "components": "rustfmt,clippy",
                "rustflags": "",
                "cache-key": "${{ inputs.cache-key }}",
            },
        )
        self.assertEqual(run["run"], 'bash "$GITHUB_ACTION_PATH/check.sh"')
        self.assertEqual(run["shell"], "bash")
        self.assertEqual(
            run["env"],
            {
                "SETUP_TOOLCHAIN": "${{ inputs.setup-toolchain }}",
                "WORKING_DIRECTORY": "${{ inputs.working-directory }}",
                "CHECKS": "${{ inputs.checks }}",
                "ALL_FEATURES": "${{ inputs.all-features }}",
                "BUILD_PROFILE": "${{ inputs.build-profile }}",
            },
        )
        workflow = yaml.safe_load((ROOT / ".github/workflows/ci.yaml").read_text())
        steps = workflow["jobs"]["check"]["steps"]
        rust_setup = next(step for step in steps if step.get("uses") == SETUP)
        self.assertEqual(
            rust_setup["with"],
            {
                "toolchain": "stable",
                "components": "rustfmt,clippy",
                "cache": False,
                "rustflags": "",
            },
        )
        self.assertEqual(
            rust_setup["if"],
            "${{ !cancelled() && (steps.changes.outcome != 'success' || "
            "fromJSON(steps.changes.outputs.changes).python) }}",
        )

    def test_real_offline_crate_checks_and_nonmutating_format_failure(self):
        environment = {
            **os.environ,
            "CARGO_HOME": str(self.root / "cargo-home"),
            "CARGO_TARGET_DIR": str(self.root / "target"),
            "CARGO_NET_OFFLINE": "true",
            "CARGO_TERM_COLOR": "never",
            "RUSTUP_AUTO_INSTALL": "0",
            "SETUP_TOOLCHAIN": "true",
            "WORKING_DIRECTORY": str(self.project),
            "CHECKS": "fmt clippy test build",
            "ALL_FEATURES": "true",
            "BUILD_PROFILE": "dev",
        }
        (self.project / "Cargo.toml").write_text(
            '[package]\nname = "rust_check_fixture"\nversion = "0.1.0"\n'
            'edition = "2021"\n\n[features]\nextra = []\n\n'
            '[lints.clippy]\nunwrap_used = "deny"\n'
        )
        config = self.project / "rustfmt.toml"
        config.write_text("max_width = 90\n")
        (self.project / "src").mkdir()
        source = self.project / "src/lib.rs"
        source.write_text(
            "/// ```\n/// assert_eq!(rust_check_fixture::answer(), 42);\n/// ```\n"
            "pub fn answer()->u8{42}\n\n"
            '#[cfg(all(test, feature = "extra"))]\nmod tests {\n'
            "    #[test]\n    fn feature_enabled() {\n"
            "        assert_eq!(super::answer(), 42);\n    }\n\n}\n"
        )
        (self.project / "examples").mkdir()
        (self.project / "examples/example.rs").write_text(
            'fn main() {\n    println!("{}", rust_check_fixture::answer());\n}\n\n'
            "#[test]\nfn example_test() {\n    assert_eq!(rust_check_fixture::answer(), 42);\n}\n"
        )
        subprocess.run(
            ["cargo", "generate-lockfile", "--offline"],
            cwd=self.project,
            env=environment,
            check=True,
            capture_output=True,
            text=True,
        )
        original = source.read_bytes()
        preserved = {
            path: path.read_bytes()
            for path in [self.project / "Cargo.toml", self.project / "Cargo.lock", config]
        }
        result = subprocess.run(
            ["bash", str(ACTION / "check.sh")],
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("lib.rs", result.stdout + result.stderr)
        self.assertEqual(source.read_bytes(), original)
        self.assertFalse((self.root / "target").exists())
        subprocess.run(
            ["cargo", "fmt", "--all"],
            cwd=self.project,
            env=environment,
            check=True,
            capture_output=True,
            text=True,
        )
        result = subprocess.run(
            ["bash", str(ACTION / "check.sh")],
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Checking rust_check_fixture", result.stderr)
        self.assertIn("tests::feature_enabled ... ok", result.stdout)
        self.assertIn("example_test ... ok", result.stdout)
        self.assertIn("Doc-tests rust_check_fixture", result.stderr)
        self.assertIn("1 passed", result.stdout)
        self.assertTrue((self.root / "target/debug/librust_check_fixture.rlib").is_file())
        for path, contents in preserved.items():
            self.assertEqual(path.read_bytes(), contents)

        environment["CHECKS"] = "build"
        environment["BUILD_PROFILE"] = "release"
        result = subprocess.run(
            ["bash", str(ACTION / "check.sh")],
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((self.root / "target/release/librust_check_fixture.rlib").is_file())

        environment["CHECKS"] = "clippy"
        source.write_text("pub fn answer() -> u8 {\n    let answer = 42;\n    answer\n}\n")
        result = subprocess.run(
            ["bash", str(ACTION / "check.sh")],
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("let_and_return", result.stderr)

        environment["CHECKS"] = "test"
        source.write_text(
            "pub fn answer() -> u8 { 42 }\n\n#[test]\nfn fails() {\n    assert_eq!(1, 2);\n}\n"
        )
        result = subprocess.run(
            ["bash", str(ACTION / "check.sh")],
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("test result: FAILED", result.stdout)

    def test_real_offline_binary_only_package(self):
        (self.project / "src").mkdir()
        (self.project / "Cargo.toml").write_text(
            '[package]\nname="binary_fixture"\nversion="0.1.0"\nedition="2021"\n'
        )
        (self.project / "src/main.rs").write_text("fn main() {}\n")
        environment = {
            **os.environ,
            "CARGO_HOME": str(self.root / "cargo-home"),
            "CARGO_TARGET_DIR": str(self.root / "target"),
            "CARGO_NET_OFFLINE": "true",
            "RUSTUP_AUTO_INSTALL": "0",
            "SETUP_TOOLCHAIN": "false",
            "WORKING_DIRECTORY": str(self.project),
            "CHECKS": "fmt clippy test build",
            "ALL_FEATURES": "true",
            "BUILD_PROFILE": "dev",
        }
        subprocess.run(
            ["cargo", "generate-lockfile", "--offline"],
            cwd=self.project,
            env=environment,
            capture_output=True,
            check=True,
        )
        result = subprocess.run(
            ["bash", str(ACTION / "check.sh")],
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((self.root / "target/debug/binary_fixture").is_file())
