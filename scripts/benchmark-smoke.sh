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

run_benchmark() {
  local package="$1"
  local bench="$2"
  local log="$3"
  shift 3
  {
    # Baseline and candidate worktrees share the target directory. Cargo can
    # otherwise reuse an executable compiled from the other revision when
    # checkout mtimes make the stale artifact appear fresh.
    # Clean only the benchmarked package so dependency builds and Iai
    # measurements remain reusable.
    CARGO_TARGET_DIR="$target_dir" cargo clean -p "$package" --release --locked
    CARGO_TARGET_DIR="$target_dir" \
      cargo bench -p "$package" --bench "$bench" --locked -- "$@"
  } 2>&1 | tee "$artifact_dir/$log"
}

base_sha="${PERF_BASE_SHA:-}"
baseline_dir=""
worktree_parent=""

cleanup() {
  if [[ -n "$baseline_dir" ]]; then
    git worktree remove --force "$baseline_dir" >/dev/null 2>&1 || true
  fi
  if [[ -n "$worktree_parent" ]]; then
    rm -rf "$worktree_parent"
  fi
}
trap cleanup EXIT

if [[ -n "$base_sha" ]]; then
  worktree_parent="$(mktemp -d)"
  baseline_dir="$worktree_parent/base"
  git worktree add --detach "$baseline_dir" "$base_sha" >/dev/null
fi

benchmark_pair() {
  local package="$1"
  local bench="$2"
  local bench_path="$3"
  local baseline_log="$4"
  local candidate_log="$5"

  # A benchmark introduced by the candidate has no comparable base identity.
  # Seed it now so future changes can compare deterministic instruction counts.
  if [[ -n "$baseline_dir" ]] && git -C "$baseline_dir" cat-file -e "HEAD:$bench_path" 2>/dev/null; then
    (
      cd "$baseline_dir"
      run_benchmark "$package" "$bench" "$baseline_log" --save-baseline=pr_base
    )
    run_benchmark "$package" "$bench" "$candidate_log" --baseline=pr_base
  else
    printf 'No compatible base benchmark exists for %s; seeding its performance-smoke contract.\n' "$package" \
      | tee "$artifact_dir/$baseline_log"
    run_benchmark "$package" "$bench" "$candidate_log" --save-baseline=seed
  fi
}

benchmark_if_present() {
  local package="$1"
  local bench="$2"
  local bench_path="$3"
  local baseline_log="$4"
  local candidate_log="$5"

  if [[ -f "$bench_path" ]]; then
    benchmark_pair "$package" "$bench" "$bench_path" "$baseline_log" "$candidate_log"
  fi
}

benchmark_if_present \
  moenarch-maps-kernels-core \
  performance_smoke \
  crates/moenarch-maps-kernels-core/benches/performance_smoke.rs \
  baseline.log \
  candidate.log

benchmark_if_present \
  moenarch-geo-clustering \
  performance_smoke \
  crates/moenarch-geo-clustering/benches/performance_smoke.rs \
  clustering-baseline.log \
  clustering-candidate.log
