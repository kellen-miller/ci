# Personal CI

Reusable GitHub Actions workflows and composite actions, licensed under MIT.

Each consumer owns its configuration, public dependencies, triggers and permissions.
Build/test/lint jobs need no private-package authentication. Publishing and deployment
use the caller's registry credentials or cloud OIDC trust. Release and maintenance
actions accept explicitly supplied GitHub tokens. No organization identity is embedded.

Use workflows, actions and the shared Renovate preset from `kellen-miller/ci`.

## Workflows

| Workflow | Operations |
| --- | --- |
| `gh-actions-lint.yaml` | actionlint plus structured workflow security checks |
| `go-test.yaml` | Go test matrices with package selection and optional race, vet and build checks |
| `gradle-check.yaml` | Java/Gradle setup and caller-selected wrapper validation tasks |
| `helm-lint.yaml` | Chart dependency build, lint/render, optional kubeconform and Trivy |
| `node-build.yaml` | Frozen install plus caller build script |
| `node-lint.yaml` | Frozen install plus caller format/check/lint scripts |
| `node-test.yaml` | Frozen install, optional Playwright, tests and failure-only artifact upload |
| `python-build.yaml` | uv package build and directory-correct distribution upload |
| `python-lint.yaml` | Separate Ruff format and lint jobs |
| `python-test.yaml` | Locked uv dependency sync and pytest |
| `renovate-config-validator.yaml` | Repository or global Renovate config validation |
| `terraform-lint.yaml` | Terraform formatting, TFLint and optional Trivy |
| `terraform-test.yaml` | Module test and environment validation matrices |
| `terraform-plan.yaml` | OIDC auth, validation, plan and updated PR comment |
| `terraform-apply.yaml` | OIDC auth, saved plan and apply; optional environment gate |
| `publish-docker-gar.yaml` | Per-platform builds, manifest assembly and keyless attestation |
| `publish-helm-oci.yaml` | Package and publish Helm charts to caller-selected GAR |
| `cosign-attest.yaml` | Registry login, SLSA predicate, keyless attestation and verification |
| `deploy-github-pages.yaml` | Node site build, artifact handoff and Pages deployment |
| `deploy-image-cloud-run.yaml` | OIDC auth and caller-configured Cloud Run deployment |
| `ci.yaml` | This repository's PR-only validation and regression tests; not reusable |

There are 20 reusable workflows, plus repository CI, stale branch maintenance,
and Rust runtime releases. Workflows remain
where they coordinate multiple operations or jobs. Checkout-plus-action wrappers
have been removed: use those actions directly. Python lint keeps separate format
and lint jobs; Helm keeps rendering, schema validation and security scanning.

The GitHub Actions and Renovate validation workflows accept `check-name` to
make consumer check labels descriptive. Defaults preserve existing check names.

On pull requests, the Renovate validator skips Node setup and validation unless
the selected `config-file`, `default.json`, or a caller workflow changes. It
compares the PR head against its merge base, including deletions and renames.
The check still reports success when validation is skipped, preserving required
checks. Non-PR calls always validate. Changes to remotely inherited presets must
be validated in the repository that owns them.

```yaml
name: Test
on:
  pull_request:
permissions:
  contents: read
concurrency:
  group: ${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: true
jobs:
  go:
    uses: kellen-miller/ci/.github/workflows/go-test.yaml@<reviewed-commit>
    with:
      working-directory: backend
```

Test artifacts configured with `artifact-name` and `artifact-path` upload only
after failure and expire after seven days by default. Set
`artifact-upload-on-success: true` when successful test output is needed, or
`artifact-retention-days` to override retention.

Set concurrency on each caller workflow so a new run cancels superseded checks
for the same workflow and PR or branch, including its reusable workflow jobs.
The shared workflows leave concurrency to callers; defining the same group in
both caller and called workflows can cancel the caller itself. Keep cancellation
disabled for release, publication, deployment and infrastructure writes. For a
workflow that only previews on PRs and writes on other events, use
`cancel-in-progress: ${{ github.event_name == 'pull_request' }}`.

Use a full reviewed commit SHA. Workflow-internal action references are also full
commit SHAs; there are no mutable internal branch references. Consumers control
when to upgrade. Third-party actions are SHA-pinned, but upstream Docker actions
can still reference image tags internally; an action pin alone does not freeze
every transitive dependency.

## Config and failure behavior

- YAML uses the same rules as Homeserver, Skills, Sup and Dotfiles: two-space
  indentation, block collections, `true`/`false` booleans, and no line-length or
  document-start requirement. `.gitignore` controls exclusions. Validation uses
  `yamllint --strict`; consumers can copy this repository's `.yamllint`.
- Go lint and formatting use `config/golangci.yaml` bundled with the selected
  action revision. Set `config` to a path relative to `working-directory` for an
  explicit replacement. Both checks use the same resolver and policy. Project
  configs are not discovered implicitly, and formatting checks do not rewrite files.
- Node resolves an explicit runtime, then `.node-version`, `.nvmrc`,
  `package.json`'s `engines.node`, or Node 24. An ambiguous package-manager lockfile
  requires an explicit choice. npm, pnpm, Yarn and Bun use frozen installs.
  pnpm security-policy overrides are opt-in; empty inputs preserve repository
  settings. `format-script` is opt-in. `check-script` and `lint-script` default to
  `check` and `lint`; set either to an empty string to skip it.
- Duplication checking uses an explicit `config` input first, then the consumer's
  `.jscpd.json`, then bundled `config/jscpd.json`. It scans the consumer working
  directory and prints the selected config path. Missing explicit configs fail
  instead of falling back. The shared defaults use 8 lines, 80 tokens, a 5%
  threshold, mild mode, Git ignores, and generic build/cache exclusions. It preserves configured
  paths, exclusions and threshold; optional overrides are explicit. A diagnostic
  report never replaces the original failure status. The Rust crate and the npm
  package named `jscpd` are different installations.
- The retained actionlint integration uses `reporter: local`, `filter_mode: nofilter`
  and `fail_level: any`. When using upstream Reviewdog actions directly, apply
  these settings explicitly to fail on findings throughout the repository.
- Workflow security validation parses YAML rather than matching comment text.
  All external action/workflow refs require full commit SHAs; Docker actions
  require SHA-256 digests; checkout must disable credential persistence.
  `pull_request_target` and secret inheritance require explicit opt-in.
  The composite action and reusable workflow expose those opt-ins for independently
  reviewed policies.
  This is a small guardrail checker, not a full execution/security analyzer.

Go tests accept a `runner` from the caller's OS matrix. Set `go-version-file` to
`go.mod` or `go.work` to use the repository's toolchain requirement; it takes
precedence over `go-version`. Race detection defaults to enabled. `vet` and
`build-target` add vet/build checks after tests without replacing consumer policy.

Gradle validation uses the consumer's committed wrapper and accepts Java version,
distribution, working directory, runner and task arguments. Arguments are split
on whitespace and passed directly to the wrapper; shell expressions are not
evaluated.

## Helm validation

The chart root can contain one chart or nested chart directories. Vendored
`charts/` and `templates/` directories are pruned relative to that root, so paths
such as `kubernetes/charts` work. Finding no charts fails.

Dependencies build before linting. Commit `Chart.lock` for reproducible dependency
resolution. Each chart is linted and rendered using its defaults and every matching
values file. Dependency, lint, render and schema failures stop the job. Trivy scans
the same successfully rendered manifests; rendering is not repeated or ignored.

Kubeconform release archives are checked against the publisher's SHA-256 manifest
before installation. The installer supports Linux and macOS, amd64 and arm64.
Missing resource schemas fail by default. Supply `schema-locations` for custom
resources or explicitly set `ignore-missing-schemas: true`. Pin custom schema URLs
and `kubernetes-version` when reproducibility is required.

## Actions

| Action | Purpose |
| --- | --- |
| `go-lint` | Go setup, golangci-lint, formatting and optional govulncheck |
| `rust-check` | Rust setup/cache and locked workspace formatting, Clippy, tests and build |
| `setup-node` | Runtime/package-manager detection, cache and frozen install |
| `helm-lint-charts` | Chart dependency build, lint/render and optional schema validation |
| `validate-workflow-security` | Structured action reference and credential policy checks |
| `code-duplication` | Rust cpd with shared defaults, local overrides and preserved failure status |
| `docker-publish-core` | Buildx/QEMU, metadata, registry login, cache, build/push and provenance |
| `playwright-setup` | Installed-version/architecture browser cache and OS dependency setup |
| `select-runner` | Prefer ready self-hosted capacity, otherwise select a hosted runner |
| `resolve-github-release` | Resolve explicit or latest public GitHub release tags |
| `gcp-gar-auth` | Caller-owned GCP OIDC auth, SDK setup and optional Docker auth |
| `github-app-auth` | Caller-scoped App token and optional repository-local bot identity |
| `terraform-plan-comment` | Create or update an environment/directory-specific PR plan comment |
| `clean-generated-output-dir` | Delete marked generated files inside an explicit workspace subtree |
| `release` | Conventional Commit releases, optional path scoping and initial version |
| `stale-branch-cleanup` | Explicit repository targets, warning period and dry-run default |

Use composite actions directly when combining checks in one job. Each action
lives under `.github/actions/<name>/action.yaml`.

`select-runner` runs in a GitHub-hosted selection job. Pass `preferred-runner`,
`fallback-runner` (default `ubuntu-24.04`) and a runner-status `token`; expose its
`runner` output as a job output and use it in dependent jobs' `runs-on`.
Runner lookup requires repository Administration read permission; the standard
`GITHUB_TOKEN` does not provide it. Without a suitable status token, lookup errors
emit a warning and select GitHub-hosted capacity. No token is logged.
Selection requires an online, idle matching runner at dispatch time. Pools scaled
to zero use hosted capacity; keep warm capacity if self-hosted preference is wanted.
Selection cannot move an already-dispatched job after a runner disappears.
Hosted jobs must have suitable network access and use reachable dependency sources.

```yaml
jobs:
  lint:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: false
      - uses: kellen-miller/ci/.github/actions/go-lint@<reviewed-action-commit>
        with:
          working-directory: backend
```

For Node, call `.github/actions/setup-node`, then run the
repository's check/lint scripts using its `package-manager` output. For Helm and
duplication use `.github/actions/helm-lint-charts` and
`.github/actions/code-duplication` directly. The shared actions are available
without the whole-job wrappers.

## Rust validation

After checkout, use `.github/actions/rust-check` directly in the consumer's job:

```yaml
- uses: kellen-miller/ci/.github/actions/rust-check@<reviewed-action-commit>
  with:
    working-directory: backend
    checks: fmt clippy test build
```

| Input | Default | Contract |
| --- | --- | --- |
| `setup-toolchain` | `'true'` | Set up Rust and its cache; `'false'` requires Rust, rustfmt and Clippy already prepared by the caller |
| `working-directory` | `.` | Cargo workspace directory relative to the checkout |
| `toolchain` | empty | Discover `rust-toolchain.toml` or `rust-toolchain` in that directory; otherwise stable |
| `checks` | `fmt clippy test build` | Space-separated checks in execution order; no empty, unknown or duplicate selections |
| `all-features` | `'true'` | Add `--all-features` to Clippy, tests and build; `'false'` preserves Cargo's default feature selection |
| `build-profile` | `dev` | Build profile, either `dev` or `release`; does not change the Clippy/test profile |
| `build-targets` | `all` | `bins` builds deployable binaries without compiling unused release test executables; Clippy and tests still cover all targets |
| `cache-key` | empty | Additional upstream cache key for consumer matrices |
| `policy-directory` | empty | Bundled Rust policy; an explicit replacement directory resolves relative to `working-directory` |

The pinned `actions-rust-lang/setup-rust-toolchain` installs rustfmt and Clippy,
discovers the caller's toolchain using `rust-src-dir`, and caches that workspace,
including failed runs. An explicit `toolchain` overrides file discovery. Empty
upstream `rustflags` preserves caller Rust flags.

The action loads the [Rust policy](config/rust/README.md) bundled at its own
revision. It passes lint levels from `lints.toml` as Clippy arguments, selects
`clippy.toml` with `CLIPPY_CONF_DIR`, and passes `rustfmt.toml` explicitly to the
formatter. Cargo manifests and project configuration files are not rewritten.
Set `policy-directory` only to replace all three policy files explicitly.

The action requires Bash and the verified Rust runtime. Rust validates inputs
and selected policy before running Cargo, rejects multiline script inputs, and
stops on the first failure.
Formatting runs `cargo fmt --all -- --check --config-path <policy>/rustfmt.toml`.
Every dependency-resolving command uses `--locked --workspace`; commit `Cargo.lock`.
Clippy adds `--all-targets`, the shared lint flags, and `-D warnings`.
When `clippy` is selected, the action first caps checked-out Rust source
candidates in handwritten workspace packages at 750 nonblank, noncomment code
lines. Rust string contents, including multiline raw strings, and code on lines
with trailing comments count; comments and blank lines do not. Checks without
`clippy` do not run this file-length check.
Tracked files remain in scope even when ignored; untracked ignored files are
excluded. Nested Cargo dependency roots and Cargo's metadata-reported target
directory are excluded. Rust symlinks to external, dependency, or generated
source fail clearly; links to handwritten workspace source stay covered.
Generated workspace members use the same explicit metadata exclusion as Clippy.
Tests use `--all-targets`, then `--doc`
when locked Cargo metadata identifies a workspace target with doctests enabled.
Binary-only workspaces skip that second command; metadata failures fail the check.
Build uses `--all-targets` and `--profile dev` or `--profile release`.

Consumers retain firmware targets, CAN checks, cross-linkers, code generation and
other project-specific setup. When that setup requires Rust earlier in the job,
use the upstream setup action there and pass `setup-toolchain: 'false'` here;
`toolchain` and `cache-key` then have no effect. This avoids a second setup/cache
registration while keeping Cargo checks after the required dependencies.

## Releases, publishing and maintenance

`release` runs directly after a full-history checkout (`fetch-depth: 0`,
`fetch-tags: true`, `persist-credentials: false`). Supply a token with
`contents: write`. Standard Conventional Commits produce major releases for
breaking changes. The former prelaunch policy is opt-in with `prelaunch: true`;
`first-release-version: 0.1.0` explicitly selects the old initial-version behavior.
Empty initial version uses semantic-release defaults. `dry-run: true` calculates
outputs without publishing. `commit-paths` scopes commits and notes; when empty,
a subdirectory release scopes to that directory. `config-file` loads an explicit
CommonJS/ESM config without replacing consumer files. `extra-plugins` accepts
public `package@version` specs. Shared tooling uses a committed public npm lockfile.
Use an App/PAT token if release events need to trigger downstream workflows;
GitHub's built-in token has event-trigger restrictions.

GHCR publication uses `docker-publish-core` directly: set `image` to
`ghcr.io/<owner>/<image>`, `registry: ghcr.io`, `registry-username` to the actor and
`registry-password` to `github.token`; grant `packages: write`. The GAR workflow
adds platform builds, artifact handoff, manifest publication and attestation.
Use distinct `artifact-prefix` values when invoking it more than once in a run.
Cloud workflows require `id-token: write`, caller-selected projects/providers and
appropriate cloud bindings. Environment gates remain under consumer control.
Cosign verification requires an exact caller-supplied signing certificate identity,
including its workflow URL and ref; there is no organization-wide regex default.

The `Stale Branch Cleanup` workflow scans this owner's installed repositories every
Monday at 09:00 UTC, following Weave's weekly warning/cleanup model. Archived and
forked repositories are excluded during discovery and checked again by the action.
Set repository variable `BRANCH_CLEANUP_APP_CLIENT_ID` and secret
`BRANCH_CLEANUP_APP_PRIVATE_KEY` for a GitHub App installed on the target
repositories with `contents: write`, `issues: write`, and `pull-requests: read`.
The schedule writes warnings and deletes eligible branches; manual runs default
to dry-run. No eligible repositories means no cleanup action runs.

`stale-branch-cleanup` is also available as a standalone action. Empty `repositories`
means only the caller repository; explicit owner/repo targets enable multiple
repositories. Default `dry-run: true` performs no branch/issue writes. Setting it
false first records warnings in an issue; deletion requires an unchanged tip and
at least `warning-days` (default 7). Default/protected branches and branches with
open PRs are skipped. Pagination covers all branches/issues, and tip/protection/PR
state is rechecked before deletion. Use `contents: write`, `issues: write` and
`pull-requests: read`;
a cross-repository App/PAT must cover every selected repository. Calls for the
same targets should use caller-defined concurrency with cancellation disabled.

`clean-generated-output-dir` requires an explicit directory strictly inside
`github.workspace`. It preserves handwritten files and symlinks and rejects the
workspace root. It deletes files marked `generated by ... do not edit` in the
first ten lines, then removes empty descendant directories.

Vault workflows/actions and private-package auth are excluded. Runners use GitHub's
hosted infrastructure; the shared dependency policy uses public registries.
Release self-tests are part of `ci.yaml` rather than a second repository workflow.

## Shared Renovate configuration

`default.json` is the shared preset. This repository's `renovate.json` extends it;
other repositories can use the same reference:

```json
{
  "extends": ["github>kellen-miller/ci"]
}
```

The preset groups non-major dependency updates, uses squash merges and `chore`
commits, maintains lockfiles, and runs `go mod tidy` after Go updates. Eligible
updates can automerge after status checks pass, during the monthly merge window.
GitHub Actions require review; version releases wait three days, while SHA/digest
updates have no age delay. npm and PyPI releases also wait three days, and build/
publish tooling requires review. Custom managers update annotated YAML versions,
Git-ref digests and Docker ARG versions. Private registry credentials, organization
package exceptions, mutable CI-reference exemptions and token-specific workarounds
are excluded. The source policy's disabled vulnerability-alert updates are retained.

`platformAutomerge: false` lets Renovate enforce the monthly window. This controls
merge timing; it does not restrict Renovate scans to once a month. See the
[automerge schedule documentation](https://docs.renovatebot.com/configuration-options/#automergeschedule).
The preset uses UTC unless the consumer sets `timezone`.

Local checks and CI validate both JSON files with the pinned Renovate validator.

## Upstream actions used directly

No wrappers are provided for Hadolint, yamllint, ShellCheck or semantic PR-title
validation. Use the upstream actions and own their inputs in the consumer:

| Purpose | Action at audited pin |
| --- | --- |
| Dockerfile lint | `reviewdog/action-hadolint@2d0eb7c86a0ddd94eb625485f4cc2730e105edd8` |
| YAML lint | `reviewdog/action-yamllint@de68272fdca5f2a961fb309e0d2e13c2eb186d9e` |
| Shell lint | `reviewdog/action-shellcheck@0722bbdb0d47f04c1b53b8734d2422ac63a45ec6` |
| PR title | `amannn/action-semantic-pull-request@48f256284bd46cdaab1048c3721360e808335d50` |

For Reviewdog, use `reporter: local`, `filter_mode: nofilter`, `fail_level: any`.
PR-title validation needs `pull-requests: read` and its GitHub token environment.
Keep consumer policy and configuration in the consuming repository.

## Rust implementation and validation

Custom action logic, workflow orchestration, maintenance commands and regression
tests live in one Cargo package. `src/main.rs` lists the supported operations;
modules own filesystem, GitHub, Rust, Node, Helm, release and publishing behavior.
Actions retain their existing inputs and outputs.

Install Rust with rustfmt and Clippy, plus `actionlint`, `yamllint`, `shellcheck`,
Helm, golangci-lint, Go, Node/npm and GitHub CLI. External ecosystem tools retain
their own runtimes; the reusable Python workflows still serve Python projects.

```bash
cargo test --locked --all-targets
cargo run --locked -- check
```

`check` applies the bundled Rust policy, YAML/workflow/shell checks, Go config
verification, the pinned Renovate validator, and release integration tests.
Regression tests use Rust fixtures and local HTTP/Git servers. They cover real
Helm rendering, Go formatting, frozen npm installs, offline Cargo checks, source
ownership/line counting, output safety, and branch warning/deletion boundaries.
Real semantic-release dry runs require the committed npm dependencies; `check`
installs them and runs those otherwise ignored Rust tests. No test publishes a
release, deletes a real branch, or uses cloud credentials.

Production JavaScript is limited to two adapters required by semantic-release's
Node API and plugin callbacks. Rust owns release orchestration, default policy,
path filtering and tests. One caller-plugin fixture exercises the Node object API.
YAML, TOML, JSON and caller release configuration remain
in the formats required by their tools. Shell steps contain short native tool
invocations; `.github/actions/run.sh` bootstraps the executable.

## Runtime binaries and action pins

External actions download the fixed `v<version>` release selected by
`runtime-version`. The loader selects Linux x64/arm64, macOS x64/arm64, or Windows
x64; verifies SHA-256 against that release's checksum record; then extracts and
executes the archive. The binary also checks its embedded Rust source identity
against the action checkout and rejects a stale runtime before running operations.
Linux binaries use musl for static linking. The job caches
archives under `runner.temp` and verifies them on every invocation. No Cargo or
Python interpreter is required for the shared Rust runtime.
The loader requires `RUNNER_TEMP`; GitHub Actions provides this private job directory.

Repository CI builds the current checkout first and explicitly supplies
`CI_BINARY`; local development can do the same. This override executes the given
binary directly, allowing PR tests without a published release. Bundled policy
and required Node adapters come from the selected action checkout through
`CI_ROOT`.

The `Release Rust runtime` workflow runs on pushes to `main` and manual dispatch
on `main`. It uses the shared Conventional Commit release action to calculate
versions, starting at `0.1.0`. A commit without a release-triggering change skips
build and publication. All five platform archives must pass native qualification
before publication. The workflow synchronizes Cargo and loader metadata with a
`chore(release): <version> [skip ci]` commit directly to `main`, then all platform
builds and publication check out that exact commit. A repository-specific SSH
key, stored as `RUNTIME_RELEASE_SSH_KEY`, bypasses PR and required-check rules
for this metadata push. `.github/rulesets/repositories.json` owns this exception;
force-push protection remains enforced. Ordinary changes still require PRs.
The skip marker prevents the metadata push from starting a second release run;
the current run continues through qualification and publication. A concurrent
update to `main` rejects the push instead of overwriting it; retry on current main.
The workflow uses its freshly built binary, so the first release does not depend
on an existing runtime. GitHub's token-generated tag does not need to trigger a
second workflow.

Consumers should pin the published release tag's commit, whose `runtime-version`
and embedded source identity match the assets. Publish the runtime assets before
upgrading consumers. Release assets and checksum records are trusted as GitHub
release content; keep those versioned assets immutable. A workflow commit pin
alone does not prevent a release owner from replacing assets.

Git path output must be UTF-8; malformed filenames fail validation. Credential
copies use mode 0600 on Unix and inherit the runner temporary directory's ACLs
on Windows. Windows script tools run through the Bash required by these actions.

After changing implementation, commit it before updating internal action pins:

```bash
git add Cargo.toml Cargo.lock build.rs src config runtime-version .github/actions
git commit -m "refactor: update CI operation"
cargo run --locked -- pin-actions HEAD
```

The pin command rejects differences in actions, Rust sources, Cargo inputs,
policy and runtime version, including untracked implementation files. It verifies
the selected snapshot contains the executable and referenced actions. Review and
validate the resulting workflow pins before publication.

## CI path gates

The `changed-paths` composite action accepts `filters`, a JSON object of group
names mapped to positive glob lists, and returns `changes`, the same keys mapped
to booleans. Run it after checkout with `fetch-depth: 0` in the caller job.
PR diffs use their merge base, with deletions and both rename paths included.
Other events select every group. No reusable workflow wrapper is needed.

Globs preserve fnmatch semantics: `*` includes slashes, and a leading `**/`
also matches root files. Negative patterns are rejected. Detection errors fail
rather than skipping checks. Callers must run their validation on detector
failure, using this condition on jobs that need `changes`:

```yaml
if: >-
  !cancelled() && (needs.changes.result != 'success' ||
  fromJSON(needs.changes.outputs.changes).code)
```

Keep required workflow triggers and check names. Include workflow files, tools,
lockfiles, configuration, generated inputs and fixtures in each owning group.
Manual/scheduled runs and release semantics remain independent of PR paths.
The shared Actions lint workflow gates lint/security steps internally; its
required check remains successful when those inputs have not changed.

This repository gates individual validation phases and their expensive tool
setup. Local `cargo run --locked -- check` calls without `CI_CHANGED_GROUPS` run
all checks. The selected group map is supplied only by the CI detector.
