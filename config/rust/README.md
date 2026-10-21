# Rust policy preset

Optional Rust lint and formatting policy. Review and commit the settings in
each consumer. The shared `rust-check` action uses the consumer's
configuration and does not download or install this preset.

## Adopt the preset

1. Merge `Cargo.lints.toml` into the package's `Cargo.toml`; it is a fragment,
   not a standalone manifest. It selects the lint levels.
2. Copy `clippy.toml` and `rustfmt.toml` to the Cargo workspace root. The former
   sets thresholds and test exceptions; the latter sets formatting.
3. Pin a project toolchain, then run the existing shared action or its commands:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-features --all-targets -- -D warnings
cargo test --locked --workspace --all-features --all-targets
```

Commit `Cargo.lock`. If features conflict, set the action's `all-features` input
to `'false'` and supply project-specific feature checks. Cargo supplies rustfmt's
parser edition; the preset pins formatting style to 2024. The supplied configuration
is exercised with stable Rust in repository tests. Older pinned toolchains
may lack options or lints and need an upgrade or a reviewed policy adjustment.

For a multi-package workspace, merge the fragment at the workspace root,
renaming `[lints.rust]` and `[lints.clippy]` to `[workspace.lints.rust]` and
`[workspace.lints.clippy]`. Every member, including a root package if present,
must explicitly inherit:

```toml
[lints]
workspace = true
```

Reconcile existing member lint tables first: members cannot combine workspace
inheritance with their own lint entries. A member without inheritance does not
receive the shared policy.

Cargo has no native external include for these lint tables. Keep reviewed copies
of the tables and config files in consumers, recording the source commit when
adopting them. A future synchronization check can report drift without rewriting
tracked files during linting. `clippy.toml` alone does not enable optional checks.

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
- [Workspace inheritance](https://doc.rust-lang.org/cargo/reference/workspaces.html#the-lints-table)
- [rustfmt options and stability](https://github.com/rust-lang/rustfmt/blob/master/Configurations.md)
