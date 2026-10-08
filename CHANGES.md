# Extraction changes

The initial package contains 12 reusable lint/validation workflows, five composite
actions, a PR-only repository validation workflow, MIT licensing and usage docs.

| Area | Implemented change |
| --- | --- |
| Authentication | Generic checks use read-only repository access; private-package setup belongs to the caller's job. |
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
| Renovate | Public recommended preset; repository validation is the default, global validation is opt-in. |
| Version boundaries | Reusable workflows reference committed action snapshots; maintenance command updates those pins explicitly. |

Deployment, release, publishing, organization maintenance, private identities and
product-specific secret schemas remain outside this lint package.

## Local evidence — October 8, 2026

`uv run python scripts/check.py` runs Ruff lint/format, yamllint, actionlint,
ShellCheck, the workflow-security checker and 16 regression tests.

Tests exercise real Helm rendering and dependency handling, real golangci-lint
formatting/config discovery, and real npm frozen installs. They also verify YAML
policy decisions, rejection of incorrect download checksums and preservation of
duplication failures.

Additional live checks downloaded kubeconform 0.8.0 with checksum verification,
validated a real ConfigMap, rejected an unknown resource schema, and accepted that
resource only with explicit missing-schema opt-in. Real cpd 5.4.0 rejected a
duplicate fixture at the caller's zero threshold and respected caller exclusions.

Hosted workflows and authenticated private-dependency wrappers have not run; the
repository is local-only pending review. The workflow definitions pass actionlint.
