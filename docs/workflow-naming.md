# Workflow naming

Display names use sentence case and preserve technical spelling. Filenames and
job IDs use kebab-case. Caller names supply the domain; reusable job names identify
the actual suite, tool, or operation. For example:

```yaml
jobs:
  go-package-tests:
    name: Go
    uses: kellen-miller/ci/.github/workflows/go-test.yaml@<reviewed-commit>
    with:
      check-name: Package tests with race detection
  browser-end-to-end:
    name: Browser
    uses: kellen-miller/ci/.github/workflows/node-test.yaml@<reviewed-commit>
    with:
      test-script: test:e2e
      check-name: Playwright end-to-end tests
```

These produce `Go / Package tests with race detection` and
`Browser / Playwright end-to-end tests`. The second example assumes the caller's
`test:e2e` script actually runs Playwright end-to-end tests.

Avoid bare `Tests`, `Lint`, `Build`, or `Checks`. Identify the tool, test scope,
build product, or validation boundary. Shared commands cannot infer whether a
consumer's package tests are unit or integration tests. `check-name` is available
on Go tests, Gradle tasks, Node.js build/validation/tests, and Python tests so the
caller can provide the precise name. Empty values retain the command-based default.
Existing GitHub Actions and Renovate `check-name` defaults stay unchanged.

Keep matrix target/environment labels parenthesized. Prefer domain caller names
without repeating the domain in the inner job. Distinguish repeated caller names
with meaningful matrix or target labels where necessary. Local jobs use
`Domain / Specific operation` for the same visible structure.

## Shared-CI migration

Reusable workflow paths, triggers, commands, permissions, output keys, and action
pins stay stable. This repository's aggregate `ci.yaml` is replaced by the domain
workflows below. Each domain runs on every PR and reports its own check. This
removes the aggregate workflow's path-based phase selection and increases job
startup/tool setup in exchange for independently visible verification results.
Renamed internal job IDs have their `needs` and output references updated.
The only new inputs are optional `check-name` strings; they affect job display only.
No CLI release or internal action repinning is needed for these YAML-only changes.

Consumers pinned to older commits keep their existing names. When upgrading, update
caller names and `.github/ruleset-requirements.json` to the exact emitted check
names, then reconcile the live ruleset after a successful run. A workflow's title
is not part of the required-check name; reusable caller and inner job names are.

This repository's required `check` becomes the individual checks listed below.
The manifest in this PR records them. Before merging, replace `check` in the
active `personal/main` ruleset with these successful checks, preserving their
GitHub Actions integration binding. The old requirement otherwise waits
indefinitely. This PR does not change live settings. A required JSCPD scan now checks the
actual repository using the shared duplication policy; the artificial threshold
fixture test is removed.

The artifact-retention caller still uses its existing SHA-pinned cleanup workflow;
its new caller label is `PR artifacts`, so it reports `PR artifacts / artifacts`
until its pin is upgraded in the consumer rollout.

## Repository domains

`ci.yaml` is removed. Its phases now have these workflow and check identities.
The local `check` command remains a convenience for running the same phases
together; no Rust implementation changes are needed.

| Workflow | Domain | Check | Required |
| --- | --- | --- | --- |
| `rust.yaml` | Rust | `Rust / Formatting validation` | Yes |
| `rust.yaml` | Rust | `Rust / Clippy` | Yes |
| `rust.yaml` | Rust | `Rust / CI CLI regression tests` | Yes |
| `yaml.yaml` | YAML | `YAML / Style validation` | Yes |
| `github-actions.yaml` | GitHub Actions | `GitHub Actions / Workflow syntax validation` | Yes |
| `github-actions.yaml` | GitHub Actions | `GitHub Actions / Security policy validation` | Yes |
| `shell.yaml` | Shell | `Shell / Static analysis` | Yes |
| `go-policy.yaml` | Go | `Go / golangci-lint configuration validation` | Yes |
| `renovate.yaml` | Renovate | `Renovate / Preset and config validation` | Yes |
| `release-validation.yaml` | Releases | `Releases / Semantic-release integration tests` | Yes |
| `code-duplication.yaml` | Code duplication | `Code duplication / JSCPD scan` | Yes |
| `code-duplication.yaml` | Code duplication | `Code duplication / Configuration tests` | Yes |

## Reusable and maintenance job-name mapping

Expressions below are evaluated by GitHub using caller inputs or matrix values.
Omitted old names fell back to job IDs. Reusable workflow paths remain stable APIs.

| Workflow file | Workflow title | Previous job ID / name | New job ID / name |
| --- | --- | --- | --- |
| `cosign-attest.yaml` | Cosign attestation | `attest` / `Cosign Attest` | `attest-image` / `Attest image with Cosign` |
| `deploy-github-pages.yaml` | GitHub Pages | `build` / `Build GitHub Pages` | `build-site` / `Build site` |
| `deploy-github-pages.yaml` | GitHub Pages | `deploy` / `Deploy GitHub Pages` | `deploy-site` / `Deploy site` |
| `deploy-image-cloud-run.yaml` | Cloud Run | `deploy-image-cloud-run` / `Deploy Image to Cloud Run` | `deploy-image` / `Deploy image to Cloud Run` |
| `gh-actions-lint.yaml` | GitHub Actions validation | `lint` / `${{ inputs.check-name }}` | `workflow-validation` / `${{ inputs.check-name }}` |
| `go-test.yaml` | Go tests | `go-test` / `Go Test` | `package-tests` / `${{ inputs.check-name \|\| format('Package tests{0}', inputs.race && ' with race detection' \|\| '') }}` |
| `gradle-check.yaml` | Gradle tasks | `check` / `Gradle Check` | `wrapper-tasks` / `${{ inputs.check-name \|\| format('Wrapper tasks ({0})', inputs.arguments) }}` |
| `helm-lint.yaml` | Helm chart validation | `lint` / `lint` | `chart-validation` / `Chart lint and rendering${{ inputs.validate-schemas && ' with schema validation' \|\| '' }}${{ inputs.trivy && ' and Trivy scan' \|\| '' }}` |
| `node-build.yaml` | Node.js build | `build` / `Node Build` | `build-script` / `${{ inputs.check-name \|\| format('Build script ({0})', inputs.build-script) }}` |
| `node-lint.yaml` | Node.js script validation | `lint` / `lint` | `validation-scripts` / `${{ inputs.check-name \|\| 'Configured format/check/lint scripts' }}` |
| `node-test.yaml` | Node.js tests | `test` / `Node Test` | `test-script` / `${{ inputs.check-name \|\| format('Test script ({0})', inputs.test-script) }}` |
| `pr-artifact-build.yaml` | PR artifact reuse | `artifacts` / `artifacts` | `resolve-artifact-reuse` / `Resolve artifact reuse` |
| `pr-artifact-build.yaml` | PR artifact reuse | `reuse` / `reuse` | `reuse-artifact` / `Reuse artifact (${{ matrix.artifact }})` |
| `pr-artifact-cleanup.yaml` | PR artifact cleanup | `artifacts` / `artifacts` | `prune-pr-artifacts` / `Prune PR artifacts` |
| `pr-artifact-retention.yaml` | PR artifact retention | `cleanup` / `cleanup` | `prune-pr-artifacts` / `PR artifacts` |
| `publish-docker-gar.yaml` | Docker images | `resolve-build-platforms` / `Resolve Build Platforms` | `resolve-image-platforms` / `Resolve image platforms` |
| `publish-docker-gar.yaml` | Docker images | `publish-platform-image` / `${{ matrix.suffix }}` | `build-platform-image` / `Build image (${{ matrix.suffix }})` |
| `publish-docker-gar.yaml` | Docker images | `publish-manifest` / `Publish Manifest` | `publish-image-manifest` / `Publish image manifest` |
| `publish-helm-oci.yaml` | Helm charts | `publish-helm-chart` / `Publish Helm Chart` | `publish-helm-chart` / `Publish Helm chart` |
| `python-build.yaml` | Python package build | `build-python-artifact` / `Build Python Artifact` | `package-distributions` / `Package distributions` |
| `python-lint.yaml` | Python formatting and lint | `format` / `Python Format` | `ruff-format` / `Ruff formatting check` |
| `python-lint.yaml` | Python formatting and lint | `lint` / `Python Lint` | `ruff-lint` / `Ruff lint` |
| `python-test.yaml` | Python tests | `python-test` / `Python Test` | `pytest-suite` / `${{ inputs.check-name \|\| 'pytest suite' }}` |
| `renovate-config-validator.yaml` | Renovate config validation | `validate` / `${{ inputs.check-name }}` | `config-validation` / `${{ inputs.check-name }}` |
| `runtime-release.yaml` | CI CLI release | `version` / `version` | `prepare-cli-version` / `Prepare CLI version` |
| `runtime-release.yaml` | CI CLI release | `build` / `build` | `build-cli-binary` / `Build CLI binary (${{ matrix.target }})` |
| `runtime-release.yaml` | CI CLI release | `publish` / `publish` | `publish-cli-release` / `Publish CLI release` |
| `stale-branch-cleanup.yaml` | Stale branches | `cleanup` / `cleanup` | `prune-stale-branches` / `Prune stale branches` |
| `terraform-apply.yaml` | Terraform apply | `apply` / `Terraform Apply (${{ inputs.env-name }})` | `infrastructure-apply` / `Infrastructure apply (${{ inputs.env-name }})` |
| `terraform-lint.yaml` | Terraform formatting and lint | `lint` / `Terraform Fmt + TFLint${{ inputs.trivy && ' + Trivy' \|\| '' }}` | `format-lint-scan` / `Formatting check and TFLint${{ inputs.trivy && ' with Trivy scan' \|\| '' }}` |
| `terraform-plan.yaml` | Terraform plan | `plan` / `Terraform Plan (${{ inputs.env-name }})` | `infrastructure-plan` / `Infrastructure plan (${{ inputs.env-name }})` |
| `terraform-test.yaml` | Terraform tests and validation | `module-tests` / `Terraform Test (${{ matrix.name }})` | `module-tests` / `Module tests (${{ matrix.name }})` |
| `terraform-test.yaml` | Terraform tests and validation | `validate` / `Terraform Validate (${{ matrix.name }})` | `environment-validation` / `Environment validation (${{ matrix.name }})` |
