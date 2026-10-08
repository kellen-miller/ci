# Personal CI

Reusable GitHub Actions workflows and composite actions, licensed under MIT.

Each consumer owns its lint configuration, dependencies and PR triggers. Checks
fail on findings and scan all relevant files, including existing issues. Generic
code checks need only `contents: read`; PR-title validation also needs
`pull-requests: read`. No custom secret or cloud identity is required.

This repository is prepared locally for `kellen-miller/ci`. It has no remote and
has not been published. Cross-repository examples become usable after publication.

## Workflows

| Workflow | Checks and configuration |
| --- | --- |
| `docker-lint.yaml` | Hadolint; repository `.hadolint.yaml` |
| `gh-actions-lint.yaml` | actionlint and structured workflow security checks |
| `go-lint.yaml` | golangci-lint, format check and govulncheck; repository Go config |
| `helm-lint.yaml` | Helm lint/render, optional kubeconform and Trivy |
| `node-lint.yaml` | Frozen package install, configurable `check` and `lint` scripts |
| `pr-lint.yaml` | Conventional Commit PR titles; configurable types/scopes/subject |
| `python-lint.yaml` | Ruff formatting and lint; repository Ruff config |
| `terraform-lint.yaml` | Terraform format, TFLint and optional Trivy |
| `yaml-lint.yaml` | yamllint; repository `.yamllint` |
| `shellcheck.yaml` | ShellCheck; configurable script glob |
| `renovate-config-validator.yaml` | Repository config by default; global config opt-in |
| `code-duplication.yaml` | Rust `jscpd` crate's `cpd`; repository `.jscpd.json` |

Workflow files document their inputs, defaults and tool versions. There is one
Helm workflow; schema and security scans are explicit inputs on that workflow.

```yaml
name: Lint
on:
  pull_request:
permissions:
  contents: read
jobs:
  go:
    uses: kellen-miller/ci/.github/workflows/go-lint.yaml@<reviewed-commit>
    with:
      working-directory: backend
  yaml:
    uses: kellen-miller/ci/.github/workflows/yaml-lint.yaml@<reviewed-commit>
```

Use a full reviewed commit SHA. Workflow-internal action references are also full
commit SHAs; there are no mutable internal branch references. Consumers control
when to upgrade. Third-party actions are SHA-pinned, but upstream Docker actions
can still reference image tags internally; an action pin alone does not freeze
every transitive dependency.

## Config and failure behavior

- Go config is discovered normally from the working directory and its parents.
  Set `config` for an explicit path relative to `working-directory`. Nothing
  downloads or replaces `.golangci.yaml`. Formatting checks fail without rewriting
  files. There is no bundled company/framework-specific preset.
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
- Reviewdog-based checks use `reporter: local`, `filter_mode: nofilter` and
  `fail_level: any`. Warnings and errors fail the job without requiring GitHub
  check-write or PR-write permissions. These defaults work on fork PRs whose
  dependency installation does not require private credentials.
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
and `kubernetes-version` when reproducibility is required. No private schema
catalog or cloud access is implicit.

## Private dependencies

Authenticate in the consuming repository before invoking a composite action in
the same job. A preparation job does not share its credentials with a reusable
workflow's separate jobs. The shared actions do not mint tokens or set Git identity.

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
      # Explicit private-dependency authentication owned by this repository goes here.
      - uses: kellen-miller/ci/.github/actions/go-lint@<reviewed-action-commit>
        with:
          working-directory: backend
```

For Node, call `.github/actions/setup-node` after authentication, then run the
repository's check/lint scripts using its `package-manager` output. For Helm and
duplication use `.github/actions/helm-lint-charts` and
`.github/actions/code-duplication` directly. The shared actions are available
without the whole-job wrappers.

## Local validation

Install `uv`, `actionlint`, `shellcheck`, `helm`, `golangci-lint`, Go and Node/npm.
The repository's own CI installs its required tools and runs the same command:

```bash
uv sync --locked
uv run python scripts/check.py
```

Tests run real Helm rendering, real Go formatting and real frozen npm installs.
Additional regressions cover YAML policy decisions, checksum rejection and
duplication failure propagation. No private repository or secret is needed.

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
