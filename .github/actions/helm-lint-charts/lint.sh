#!/usr/bin/env bash
set -euo pipefail

charts_root="$(cd "${CHARTS_ROOT:?Set CHARTS_ROOT}" && pwd)"
mkdir -p "$RENDERED_DIR"
chart_files=()
while IFS= read -r -d '' chart; do
  chart_files+=("$chart")
done < <(
  cd "$charts_root"
  find . -type d \( -name charts -o -name templates \) -prune \
    -o -type f -name Chart.yaml -print0
)

if [[ ${#chart_files[@]} -eq 0 ]]; then
  echo "No charts found under $CHARTS_ROOT" >&2
  exit 1
fi

schema_args=(-strict -summary)
if [[ "${IGNORE_MISSING_SCHEMAS:-false}" == true ]]; then
  schema_args+=(-ignore-missing-schemas)
fi

while IFS= read -r location; do
  [[ -z "$location" ]] || schema_args+=(-schema-location "$location")
done <<< "${SCHEMA_LOCATIONS:-default}"

if [[ -n "${KUBERNETES_VERSION:-}" ]]; then
  schema_args+=(-kubernetes-version "$KUBERNETES_VERSION")
fi

if [[ -n "${KUBECONFORM_CACHE:-}" ]]; then
  mkdir -p "$KUBECONFORM_CACHE"
  schema_args+=(-cache "$KUBECONFORM_CACHE")
fi

index=0
for chart in "${chart_files[@]}"; do
  chart_dir="$charts_root/$(dirname "$chart")"
  echo "Checking $chart_dir"
  if [[ -f "$chart_dir/Chart.lock" ]] || grep -q '^dependencies:' "$chart_dir/Chart.yaml"; then
    helm dependency build "$chart_dir"
  fi

  values_files=('')
  while IFS= read -r -d '' values; do
    values_files+=("$values")
  done < <(find "$chart_dir" -maxdepth 1 -type f -name "$VALUES_PATTERN" -print0)

  for values in "${values_files[@]}"; do
    value_args=("$chart_dir")
    if [[ -n "$values" ]]; then
      value_args+=(-f "$values")
    fi

    helm lint "${value_args[@]}"
    rendered="$RENDERED_DIR/$index.yaml"
    helm template lint "${value_args[@]}" > "$rendered"
    if [[ "${VALIDATE_SCHEMAS:-true}" == true ]]; then
      kubeconform "${schema_args[@]}" "$rendered"
    fi

    index=$((index + 1))
  done
done
