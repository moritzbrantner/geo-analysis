"""Exercise the real runner with Git worktrees and a stale-artifact Cargo double.

No Rust/Valgrind installation or timing thresholds are needed. The double models
Cargo accepting a shared executable as fresh; omitting the package clean makes
it run baseline code as the candidate and the regression test fails.
"""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


RUNNER = Path(__file__).resolve().parents[1] / "benchmark-smoke.sh"
BENCH = Path("crates/moenarch-maps-kernels-core/benches/performance_smoke.rs")

CARGO_DOUBLE = r'''
import os
from pathlib import Path
import sys

args = sys.argv[1:]
if args == ["-V"]:
    print("cargo fixture")
    raise SystemExit(0)
target = Path(os.environ["CARGO_TARGET_DIR"])
target.mkdir(parents=True, exist_ok=True)
compiled = target / "compiled-kernel"
phase = Path("crates/moenarch-maps-kernels-core/benches/performance_smoke.rs").read_text().strip()
with (target / "calls").open("a") as calls:
    calls.write(f"{args[0]} {phase}\n")
if args[0] == "clean":
    assert args == ["clean", "-p", "moenarch-maps-kernels-core", "--release", "--locked"]
    compiled.unlink(missing_ok=True)
    raise SystemExit(0)
assert args[:7] == ["bench", "-p", "moenarch-maps-kernels-core", "--bench", "performance_smoke", "--locked", "--"]
assert len(args) == 8
if not compiled.exists():
    compiled.write_text(phase)
executed = compiled.read_text()
print(f"executed={executed}; requested={phase}", flush=True)
if executed != phase:
    print("stale executable was reused", file=sys.stderr)
    raise SystemExit(42)
if os.environ.get("FAIL_PHASE") == phase:
    raise SystemExit(71 if phase == "baseline" else 72)
option = args[-1]
if option.startswith("--save-baseline="):
    (target / option.split("=", 1)[1]).write_text(executed)
else:
    assert option == "--baseline=pr_base"
    assert (target / "pr_base").read_text() == "baseline"
'''


class BenchmarkRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "repo"
        self.root.mkdir()
        self.git("init", "--quiet", "--initial-branch=main")
        self.git("config", "user.name", "Benchmark fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        (self.root / ".gitignore").write_text("target/\n.artifacts/\n")
        (self.root / "Cargo.lock").write_text("fixture lockfile\n")
        (self.root / "scripts").mkdir()
        shutil.copyfile(RUNNER, self.root / "scripts/benchmark-smoke.sh")
        self.pre_bench = self.commit("before benchmarks")
        (self.root / BENCH).parent.mkdir(parents=True)
        (self.root / BENCH).write_text("baseline\n")
        self.base = self.commit("baseline")
        (self.root / BENCH).write_text("candidate\n")
        self.commit("candidate")
        self.bin = Path(self.temp.name) / "bin"
        self.bin.mkdir()
        self.executable("cargo", f"#!{sys.executable}\n" + CARGO_DOUBLE)
        for tool in ("rustc", "valgrind"):
            self.executable(tool, f"#!/bin/sh\nprintf '{tool} fixture\\n'\n")
        self.target = self.root / "target/performance-smoke"
        self.target.mkdir(parents=True)
        # Both an unrelated dependency and an old executable already exist.
        (self.target / "dependency-build").write_text("must survive")
        (self.target / "compiled-kernel").write_text("stale cached revision")

    def git(self, *args):
        return subprocess.check_output(
            ["git", *args], cwd=self.root, text=True, stderr=subprocess.PIPE
        ).strip()

    def commit(self, message):
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", message)
        return self.git("rev-parse", "HEAD")

    def executable(self, name, content):
        path = self.bin / name
        path.write_text(content)
        path.chmod(0o755)

    def run_smoke(self, base=None, fail_phase=""):
        env = os.environ.copy()
        env.update(
            PATH=str(self.bin) + os.pathsep + env["PATH"],
            PERF_BASE_SHA=base or "",
            FAIL_PHASE=fail_phase,
        )
        return subprocess.run(
            ["bash", "scripts/benchmark-smoke.sh"],
            cwd=self.root, env=env, text=True, capture_output=True, check=False,
        )

    def assert_worktree_cleaned(self):
        self.assertEqual(self.git("worktree", "list", "--porcelain").count("worktree "), 1)
        self.assertEqual((self.target / "dependency-build").read_text(), "must survive")

    def test_base_and_candidate_run_their_own_code_and_keep_measurements(self):
        result = self.run_smoke(self.base)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(
            (self.target / "calls").read_text().splitlines(),
            ["clean baseline", "bench baseline", "clean candidate", "bench candidate"],
        )
        self.assertEqual((self.target / "pr_base").read_text(), "baseline")
        self.assertEqual((self.target / "compiled-kernel").read_text(), "candidate")
        log = self.root / ".artifacts/performance-smoke/candidate.log"
        self.assertIn("executed=candidate; requested=candidate", log.read_text())
        self.assert_worktree_cleaned()

    def test_seed_run_also_rejects_a_warm_stale_binary(self):
        result = self.run_smoke()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual((self.target / "seed").read_text(), "candidate")
        self.assert_worktree_cleaned()

    def test_base_without_benchmark_seeds_candidate(self):
        result = self.run_smoke(self.pre_bench)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual((self.target / "seed").read_text(), "candidate")
        self.assert_worktree_cleaned()

    def test_baseline_failure_is_not_hidden_by_tee(self):
        result = self.run_smoke(self.base, fail_phase="baseline")
        self.assertEqual(result.returncode, 71, result.stdout + result.stderr)
        self.assertFalse((self.root / ".artifacts/performance-smoke/candidate.log").exists())
        self.assert_worktree_cleaned()

    def test_candidate_failure_is_not_hidden_by_tee(self):
        result = self.run_smoke(self.base, fail_phase="candidate")
        self.assertEqual(result.returncode, 72, result.stdout + result.stderr)
        self.assert_worktree_cleaned()


if __name__ == "__main__":
    unittest.main()
