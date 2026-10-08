# Extraction changes

The package contains 19 reusable workflows, 14 composite actions, one PR-only
repository CI workflow, MIT licensing and usage docs. Single-action workflow
wrappers are removed; consumers invoke those actions directly.

| Area | Implemented change |
| --- | --- |
| Dependencies | Public registries/repositories; no private-package authentication inputs or setup. |
| Configuration | Go, Ruff, YAML, Terraform, Node and duplication policies remain in each consumer. |
| Go | Normal config discovery or explicit relative path; formatting differences fail without rewriting files. |
| Node | Lockfile detection, real runtime-file resolution, frozen installs, caller-owned pnpm policy; Yarn does not auto-edit package metadata. |
| Reviewdog | Local reporting, all-file filtering and explicit failure on any finding. |
| Helm | One workflow; discover relative to root, build dependencies first, check defaults plus environment values, reject empty roots. |
| Schema validation | Explicit missing-schema policy; checksum-verified kubeconform installs on Linux/macOS and amd64/arm64. |
| Trivy | Scan successfully rendered manifests in the same job; dependency/render failures stop the scan. |
| Workflow security | Parse YAML; require SHA-pinned refs, digest-pinned Docker actions and disabled checkout credential persistence. |
| Security policy | Explicit opt-in for privileged PR triggers or inherited secrets; no organization-specific ref exemptions. |
| Duplication | Configurable Rust crate version, caller config/exclusions/threshold, original failure preserved after diagnostic output. |
| Renovate | Shared public preset with grouping, monthly automerge, age gates and custom managers; repository validation by default. |
| Version boundaries | Reusable workflows reference committed action snapshots; maintenance command updates those pins explicitly. |

Build/test, publishing, signing, deployment, release and branch maintenance are now
included. Vault and private-package authentication remain excluded. GitHub App
identity becomes explicit `github-app-auth` inputs; cloud targets and signer trust
are supplied by the consumer. No automatic branch cleanup schedule is installed.

Additional changes:

- Python build artifacts honor the selected working directory; pytest arguments
  are passed without evaluating shell syntax; dependency sync checks the lockfile.
- Docker cache source/destination overrides work independently. Build secrets use
  one build step; GAR digest artifacts accept distinct invocation prefixes.
- GCP credential files move outside the workspace before builds or artifact uploads.
- Playwright cache keys include architecture, and both hit/miss paths install OS deps.
- Terraform plan comments update an existing directory/environment report.
- Generated-output cleanup rejects the workspace root and preserves symlinks.
- Release configuration stays inside the pinned action; caller configs are never
  overwritten. Breaking changes are major by default; prelaunch/initial-version
  overrides are explicit. Public tooling has a committed npm lockfile.
- Branch cleanup paginates explicit targets, skips protected/default/open-PR branches,
  requires elapsed warnings and an unchanged tip, and defaults to no writes.
- Optional `config/golangci.yaml` removes organization prefixes/framework exceptions
  and broad security exclusions; Go consumers still own their configuration.

## Local evidence — October 8, 2026

`uv run python scripts/check.py` runs Ruff lint/format, yamllint, actionlint,
Go configuration verification, strict Renovate configuration validation,
ShellCheck, the workflow-security checker and 21 Python regression tests plus
eight release tests.

The shared Renovate preset also passed resolved-policy checks for eight dependency
cases and real extraction checks for all three custom managers with native RE2.

Tests exercise real Helm rendering and dependency handling, real golangci-lint
formatting/config discovery, and real npm frozen installs. They also verify YAML
policy decisions, rejection of incorrect download checksums and preservation of
duplication failures.

Additional live checks downloaded kubeconform 0.8.0 with checksum verification,
validated a real ConfigMap, rejected an unknown resource schema, and accepted that
resource only with explicit missing-schema opt-in. Real cpd 5.4.0 rejected a
duplicate fixture at the caller's zero threshold and respected caller exclusions.

Hosted workflows have not run; the repository is local-only pending review.
The workflow definitions pass actionlint.
