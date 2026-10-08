# Personal CI

Reusable GitHub Actions workflows and composite actions, licensed under MIT.

Each consumer owns its configuration, public dependencies, triggers and permissions.
Build/test/lint jobs need no private-package authentication. Publishing and deployment
use the caller's registry credentials or cloud OIDC trust. Release and maintenance
actions accept explicitly supplied GitHub tokens. No organization identity is embedded.

This repository is prepared locally for `kellen-miller/ci`. It has no remote and
has not been published. Cross-repository examples become usable after publication.

## Workflows

| Workflow | Operations |
| --- | --- |
| `gh-actions-lint.yaml` | actionlint plus structured workflow security checks |
| `go-test.yaml` | Go tests with optional race and integration mode |
| `helm-lint.yaml` | Chart dependency build, lint/render, optional kubeconform and Trivy |
| `node-build.yaml` | Frozen install plus caller build script |
| `node-lint.yaml` | Frozen install plus caller check/lint scripts |
| `node-test.yaml` | Frozen install, optional Playwright, tests and artifact upload |
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

There are 19 reusable workflows and one repository CI workflow. Workflows remain
where they coordinate multiple operations or jobs. Checkout-plus-action wrappers
have been removed: use those actions directly. Python lint keeps separate format
and lint jobs; Helm keeps rendering, schema validation and security scanning.

```yaml
name: Test
on:
  pull_request:
permissions:
  contents: read
jobs:
  go:
    uses: kellen-miller/ci/.github/workflows/go-test.yaml@<reviewed-commit>
    with:
      working-directory: backend
```

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
- Go config is discovered normally from the working directory and its parents.
  Set `config` for an explicit path relative to `working-directory`. Nothing
  downloads or replaces `.golangci.yaml`. Formatting checks fail without rewriting
  files. `configs/golangci.yaml` is an optional generalized preset; copy and review
  it explicitly. It is never downloaded into a consumer automatically.
- Node resolves an explicit runtime, then `.node-version`, `.nvmrc`,
  `package.json`'s `engines.node`, or Node 24. An ambiguous package-manager lockfile
  requires an explicit choice. npm, pnpm, Yarn and Bun use frozen installs.
  pnpm security-policy overrides are opt-in; empty inputs preserve repository
  settings. `check-script` and `lint-script` default to `check` and `lint`;
  set either to an empty string to skip it.
- Duplication checking requires an existing configuration. It preserves configured
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
  The composite action exposes those opt-ins for independently reviewed policies.
  This is a small guardrail checker, not a full execution/security analyzer.

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
| `setup-node` | Runtime/package-manager detection, cache and frozen install |
| `helm-lint-charts` | Chart dependency build, lint/render and optional schema validation |
| `validate-workflow-security` | Structured action reference and credential policy checks |
| `code-duplication` | Rust cpd with caller configuration and preserved failure status |
| `docker-publish-core` | Buildx/QEMU, metadata, registry login, cache, build/push and provenance |
| `playwright-setup` | Installed-version/architecture browser cache and OS dependency setup |
| `resolve-github-release` | Resolve explicit or latest public GitHub release tags |
| `gcp-gar-auth` | Caller-owned GCP OIDC auth, SDK setup and optional Docker auth |
| `github-app-auth` | Caller-scoped App token and optional repository-local bot identity |
| `terraform-plan-comment` | Create or update an environment/directory-specific PR plan comment |
| `clean-generated-output-dir` | Delete marked generated files inside an explicit workspace subtree |
| `release` | Conventional Commit releases, optional path scoping and initial version |
| `stale-branch-cleanup` | Explicit repository targets, warning period and dry-run default |

Use composite actions directly when combining checks in one job. Each action
lives under `.github/actions/<name>/action.yaml`.

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

`stale-branch-cleanup` is an action with no automatic schedule. Empty `repositories`
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

Vault workflows/actions and private-package auth are excluded. Organization
RunsOn configuration and private Renovate presets are replaced by hosted runners
and public configuration. The generalized Go preset is optional. Release self-tests
are part of `ci.yaml` rather than a second repository workflow.

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

## Local validation

Install `uv`, `actionlint`, `shellcheck`, `helm`, `golangci-lint`, Go and Node/npm.
The repository's own CI installs its required tools and runs the same command:

```bash
uv sync --locked
uv run python scripts/check.py
```

Tests run real Helm rendering, real Go formatting and real frozen npm installs.
Additional regressions cover YAML policy, checksum rejection, duplication failures,
generated-file cleanup, cache overrides and branch warning/deletion boundaries.
Release tests use temporary local Git repositories, including real semantic-release
dry runs. Hosted publishing/deployment/authentication has not run.

When changing shared actions, commit them before updating workflow pins:

```bash
git add .github/actions
git commit -m "fix: correct lint execution"
uv run python scripts/pin-actions.py HEAD
# Review the workflow pin changes, validate, then commit them.
```

The pin command refuses action changes that differ from the selected commit.
The action snapshot must be included in the eventual pushed history. Publish and
release only after local review; this checkout does not configure or push a remote.
