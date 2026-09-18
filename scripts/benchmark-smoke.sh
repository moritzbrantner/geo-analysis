#!/usr/bin/env bash
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

artifact_dir="$root/.artifacts/performance-smoke"
target_dir="$root/target/performance-smoke"
mkdir -p "$artifact_dir" "$target_dir"

{
  printf 'repository=%s\n' "$(git remote get-url origin 2>/dev/null || printf unknown)"
  printf 'candidate=%s\n' "$(git rev-parse HEAD)"
  printf 'baseline=%s\n' "${PERF_BASE_SHA:-none}"
  printf 'rustflags=%s\n' "${RUSTFLAGS:-}"
  printf 'cargo_target_dir=%s\n' "$target_dir"
  if [[ -f Cargo.lock ]]; then
    printf 'cargo_lock_sha256=%s\n' "$(sha256sum Cargo.lock | cut -d' ' -f1)"
  else
    printf 'cargo_lock=absent\n'
  fi
  rustc -vV
  cargo -V
  valgrind --version
  printf 'iai-callgrind-runner=%s\n' '0.16.1'
  uname -srm
} > "$artifact_dir/fingerprint.txt"

run_candidate() {
  local package="$1"
  local bench="$2"
  local log="$3"
  shift 3
  CARGO_TARGET_DIR="$target_dir" \
    cargo bench -p "$package" --bench "$bench" --locked -- "$@" \
    2>&1 | tee "$artifact_dir/$log"
}

base_sha="${PERF_BASE_SHA:-}"
baseline_dir=""
if [[ -n "$base_sha" ]]; then
  worktree_parent="$(mktemp -d)"
  baseline_dir="$worktree_parent/base"

  cleanup() {
    git worktree remove --force "$baseline_dir" >/dev/null 2>&1 || true
    rm -rf "$worktree_parent"
  }
  trap cleanup EXIT

  git worktree add --detach "$baseline_dir" "$base_sha" >/dev/null
fi

run_baseline() {
  local package="$1"
  local bench="$2"
  local log="$3"
  shift 3
  (
    cd "$baseline_dir"
    CARGO_TARGET_DIR="$target_dir" \
      cargo bench -p "$package" --bench "$bench" --locked -- "$@"
  ) 2>&1 | tee "$artifact_dir/$log"
}

benchmark_pair() {
  local package="$1"
  local bench="$2"
  local bench_path="$3"
  local baseline_log="$4"
  local candidate_log="$5"

  if [[ -n "$baseline_dir" ]] && git -C "$baseline_dir" cat-file -e "HEAD:$bench_path" 2>/dev/null; then
    run_baseline "$package" "$bench" "$baseline_log" --save-baseline=pr_base
    run_candidate "$package" "$bench" "$candidate_log" --baseline=pr_base
  else
    printf 'No compatible base benchmark exists for %s; seeding its performance contract.\n' "$package" \
      | tee "$artifact_dir/$baseline_log"
    run_candidate "$package" "$bench" "$candidate_log" --save-baseline=seed
  fi
}

benchmark_pair \
  moenarch-maps-kernels-core \
  performance_smoke \
  crates/moenarch-maps-kernels-core/benches/performance_smoke.rs \
  baseline.log \
  candidate.log

benchmark_pair \
  moenarch-geo-clustering \
  performance_smoke \
  crates/moenarch-geo-clustering/benches/performance_smoke.rs \
  clustering-baseline.log \
  clustering-candidate.log
