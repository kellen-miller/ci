# Rust policy

The shared `rust-check` action applies this policy by default. The action and
policy come from the same pinned revision; consumers do not copy configuration
or add lint tables to Cargo manifests.

## Run the policy

After checkout, invoke the shared action:

```yaml
- uses: kellen-miller/ci/.github/actions/rust-check@<reviewed-action-commit>
  with:
    checks: fmt clippy test build
```

The script reads `lints.toml` and passes compiler and Clippy lint levels as
arguments. Groups precede individual lints in that file so later entries can
refine group settings. Valid levels are `allow`, `warn`, `deny`, and `forbid`.
It sets `CLIPPY_CONF_DIR` to this directory and passes an explicit rustfmt config
path. These settings apply to every selected workspace member without Cargo
lint inheritance. Normal Rust source-attribute precedence still applies.

Project `clippy.toml` and `rustfmt.toml` files do not replace these defaults.
For an explicit replacement, set `policy-directory` to a directory relative to
`working-directory` containing `lints.toml`, `clippy.toml`, and `rustfmt.toml`.
The override replaces the policy; configurations are not merged. Missing required
files and invalid lint-level entries fail before any Cargo command runs. Clippy
and rustfmt validate their own configuration files.

Local checks can invoke the same script from an existing policy checkout:

```bash
SETUP_TOOLCHAIN=false WORKING_DIRECTORY=. CHECKS='fmt clippy' \
  bash /path/to/ci/.github/actions/rust-check/check.sh
```

Use Bash, Python 3.11+, and Cargo with rustfmt and Clippy installed. Pin the
checkout to the same revision as CI. The script does not install a local
toolchain or rewrite project files. Native `cargo clippy` and `cargo fmt`
commands alone do not load this external policy.

Commit `Cargo.lock`. If features conflict, set the action's `all-features` input
to `'false'` and supply project-specific feature checks. Cargo supplies rustfmt's
parser edition; the preset pins formatting style to 2024. The supplied configuration
is exercised with stable Rust in repository tests. Older pinned toolchains
may lack options or lints and need an upgrade or a reviewed policy adjustment.

## Policy coverage

| Intent | Enforcement |
| --- | --- |
| Correctness, performance, and idiomatic code | Compiler plus Clippy's default `all` group |
| Handle results, including blank assignments | `unused_must_use`, `let_underscore_must_use`, `unused_result_ok` |
| Readable control flow | `too_many_lines=80`, `cognitive_complexity=20`, nesting threshold 5 |
| Exhaustive matches | Compiler requires exhaustive matches; wildcard arms intentionally cover other variants |
| Restrict APIs | Configurable disallowed methods, types, and macros; no dependency-graph or module-boundary enforcement |
| Unfinished code | `todo!()` and `unimplemented!()` checks; ordinary TODO comments need a separate check |
| Consistent formatting | rustfmt, sorted imports, 120-column target |
| Test exclusions | Central unwrap/expect exceptions for recognized test functions and modules |
| Resource cleanup | Ownership/Drop covers many cleanup cases, not protocol completion or all errors |
| Application lifecycle | No general checks for application cancellation, logging context, span lifecycle, or useful error context |
| Duplication | Existing `code-duplication` action supports Rust independently of Clippy |

Cognitive complexity and line counts do not measure whether helper extraction
improves readability. Calibrate
the limits against real code; prefer changing shared policy over creating chains
of single-use helpers or adding inline suppressions. Nesting counts include
function, impl, and module blocks, which is why its initial threshold is 5.

`unreachable_pub` keeps implementation APIs inside the crate. The preset does
not enable `redundant_pub_crate`, which conflicts with explicit crate visibility
inside private modules. Modules need not become public to satisfy linting.

The preset selects individual pedantic/restriction checks. It does not enable
entire pedantic, restriction, or nursery groups. Production unwrap/expect checks
do not prove panic freedom; deliberately discarded errors still need review.
The test exceptions apply only where Clippy recognizes test context, not every
function in a file beneath `tests/`.

Disallowed-API lists start empty: add fully qualified paths and reasons only when
a project has an actual replacement policy. No blanket unsafe-code ban is
assumed; the preset requires explicit unsafe operations and safety comments.

Domain-owned modules, visible side effects, explicit dependencies, and avoiding
`helpers`/`utils` modules remain review rules unless implemented as custom checks.
Stable rustfmt cannot enforce a blank line after every logically complete `}`.
Even nightly blank-line bounds concern items, not that full semantic rule.
Some import grouping and comment wrapping options also require nightly rustfmt.
The width is a formatting target, not a strict limit on every comment or string.

## Sources

- [Clippy configuration and lint selection](https://doc.rust-lang.org/clippy/configuration.html)
- [Clippy groups](https://doc.rust-lang.org/clippy/lints.html)
- [Clippy threshold and exception settings](https://doc.rust-lang.org/clippy/lint_configuration.html)
- [Individual Clippy checks](https://clippy.rust-lang.org/master/index.html)
- [Cargo lint tables](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section)
- [rustfmt options and stability](https://github.com/rust-lang/rustfmt/blob/master/Configurations.md)
