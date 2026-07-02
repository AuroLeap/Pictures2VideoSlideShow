#!/usr/bin/env python3
"""The check harness — one command that runs every quality gate locally and in CI.

Stack-agnostic kit, **Python reference implementation**. This is the runnable
version of the "harness contract" in `process.md §7`: format · lint · tests ·
coverage · traceability · doc-navigability · perf-budgets · architecture-map
freshness. Wire it to your stack by
editing the step list the `steps()` function returns below — and the
`SRC`/`TESTS`/tool names in the "EDIT FOR YOUR STACK" block just under the
imports (swap `ruff`/`pytest` for your toolchain); the contract is the *gates and
exit code*, not the specific tools. For a non-Python project, replace the
format/lint/test commands with your own (or drop the ones you don't have); keep
the traceability/flows/doc-navigability/perf-budgets/arch-map steps — they're
stdlib-only and stack-agnostic.

Design choices that keep it honest and CI-friendly:
    - **Never a false green.** Any failing required step makes the whole run exit
      nonzero. We print the real command output; we do not summarize it away.
    - **Missing tool != pass.** If a step's required module isn't importable the
      step is reported SKIP(missing) and (outside --lenient) fails the run, so CI
      can't silently skip linting.
    - **One interpreter.** Tools run as `python -m ruff` / `python -m pytest` with
      the same interpreter running this script, so the launchers' venv python is
      enough — no activated venv or PATH entry required.
    - **Gate-scoped.** `--gate G2` runs only what that gate needs (e.g. G2 needs
      traceability + a runnable harness; G3 needs the full suite). Default runs all.
    - **Tiered tests.** `--tier smoke` runs only the fast subset so you can check
      every iteration; `release` runs everything including slow/hardware tests.
      Tiers map to pytest markers (`-m`); the `Tier` column in test-cases.csv is
      the registry source of truth. An **unmarked test runs in `full` and above**,
      so a forgotten marker can never drop a test from the pre-merge suite. The
      coverage threshold applies at `full`/`release` only — the smoke subset alone
      isn't expected to meet it. CI typically runs `smoke` on push, `full` on PR,
      and `release`/`all` on a release tag.
    - **Non-interactive.** No prompts; deterministic exit codes for automation.

Usage:
    python scripts/check.py [--gate G1|G2|G3|all] [--tier smoke|full|release|all]
                            [--coverage N] [--phase LIST] [--lenient] [--list]

    --gate      Which gate's checks to run. Default: the repo's **active gate**
                from the one-line `docs/gate` file (bootstrap starts it at G1;
                closing a gate = the human bumps it in a reviewed commit), else
                `all` when no gate file exists. This is what keeps a young
                project's CI green-and-honest: it enforces the bar the project
                is actually at, not the end-state bar. G3 (and all) also
                requires every Verification=Test SR to be Status=Verified
                (trace.py --require-verified).
    --tier      Which test tier to run (default: all). Mark fast critical-path
                tests @pytest.mark.smoke and expensive ones @pytest.mark.release
                (markers registered in pytest.ini); leave ordinary tests unmarked —
                they run in the full/release tiers automatically.
    --coverage  Line-coverage threshold percent (default: 80; see COVERAGE_THRESHOLD).
                Enforced for the full/release/all tiers, not smoke.
    --lenient   Treat missing tools as SKIP instead of failure (local dev only).
    --list      Print the step plan for the gate and exit; each step is tagged
                [process] (kit-owned, stdlib, identical everywhere) or [product]
                (language-specific — you wire it to your stack). See process.md
                §7 "process vs product checks".
"""

import argparse
import importlib.util
import subprocess
import sys
import time
from pathlib import Path


def _utf8_console():
    """Emit UTF-8 to stdout/stderr whatever the OS console codepage is, so a
    non-ASCII step name / path / child-process banner can't raise
    UnicodeEncodeError on a legacy Windows cp1252 console. Python 3.7+ streams
    expose `.reconfigure`; guard for the rest."""
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8")
        except (AttributeError, ValueError):
            pass


# ============================ EDIT FOR YOUR STACK ============================
# The stack-specific knobs live here. For a non-Python project: point SRC/TESTS
# at your layout, then swap the format/lint/tests commands in steps() (search
# "EDIT FOR YOUR STACK" again) for your toolchain — or drop a step you don't
# have. The traceability, design-flows, and arch-map steps are stdlib-only and
# stack-agnostic; keep them as-is.
SRC = "src"  # source root
TESTS = "tests"  # test root
COVERAGE_THRESHOLD = 80  # line-coverage %, enforced at full/release (process.md)
# ============================================================================

# Tier -> pytest marker expression. Tiers are cumulative, and the safe default
# is opt-OUT: an unmarked test runs in `full` and `release`, so forgetting a
# marker can't silently drop a test from the pre-merge suite. `smoke` is opt-in
# (mark the fast critical paths); marking `release` opts a test out of pre-merge.
TIERS = {
    "smoke": "smoke",
    "full": "not release",
    "release": None,
    "all": None,
}

# Tiers whose pytest run must meet the coverage threshold. Smoke runs only a
# subset of the tests, so holding it to the full-suite threshold would fail the
# cheap gate for the wrong reason.
COVERAGE_TIERS = ("full", "release", "all")


# Each step: name, the third-party module(s) it needs (importable by THIS
# interpreter; () = stdlib-only), the command, the set of gates that require it,
# and its layer — "process" (kit-owned, stdlib-only, identical in every project:
# traceability / design-flows / arch-map) or "product" (language-specific, you
# wire it to your stack: format / lint / tests). The empty-vs-nonempty `requires`
# tuple already implies the split; the layer tag formalizes and surfaces it (see
# process.md §7 "process vs product checks"). Edit commands to fit your stack;
# keep the gate tags and layers.
def steps(coverage, tier, gate, phase=None):
    # --- EDIT FOR YOUR STACK: the format/lint/test commands -------------------
    # `pytest_cmd` (assembled here because it varies by tier/coverage) and the
    # `ruff` format/lint entries in the returned list are the Python-reference
    # toolchain. Replace them with your stack's equivalents — or drop a step you
    # don't have — but keep each step's gate tags. Tools run as `python -m <mod>`
    # via this interpreter, so the launcher's venv python is enough (no PATH/venv
    # dance). `pytest_needs` lists the modules a step imports, so a missing tool
    # is reported SKIP(missing) and (outside --lenient) fails rather than passing.
    pytest_cmd = [sys.executable, "-m", "pytest", "-q"]
    pytest_needs = ("pytest",)
    if tier in COVERAGE_TIERS:
        pytest_cmd += [
            "--cov=" + SRC,
            "--cov-report=term-missing",
            "--cov-fail-under=" + str(coverage),
        ]
        pytest_needs = ("pytest", "pytest_cov")
    marker = TIERS.get(tier)
    if marker:
        pytest_cmd += ["-m", marker]
    # The traceability step only runs at G2/G3, where placeholder rows must be
    # gone, so --no-placeholders is always on here (a fresh scaffold is exempt
    # only because nothing past G1 runs against it). --html also regenerates the
    # scalable full-graph view (a gitignored composite artifact) every run.
    trace_cmd = [
        sys.executable,
        "scripts/trace.py",
        "--strict",
        "--no-placeholders",
        "--html",
    ]
    if gate in ("G3", "all"):  # G3 criterion: test-verifiable SRs are Verified
        trace_cmd.append("--require-verified")
        trace_cmd.append("--strict-schema")  # G3: required fields + valid enums
        if phase:  # phased delivery: close G3 for this phase only (process.md §4)
            trace_cmd += ["--phase", phase]
    return [
        # --- product checks: language-specific, wired to your stack -----------
        (
            "format",
            ("ruff",),
            [sys.executable, "-m", "ruff", "format", "--check", SRC, TESTS],
            {"G3"},
            "product",
        ),
        (
            "lint",
            ("ruff",),
            [sys.executable, "-m", "ruff", "check", SRC, TESTS],
            {"G3"},
            "product",
        ),
        ("tests+coverage", pytest_needs, pytest_cmd, {"G3"}, "product"),
        # Optional PRODUCT-layer detector, not wired into the required floor:
        # `scripts/check_stubs.py` is the Python-reference tripwire for the G3
        # no-stub / substance criterion (process.md §4). It is warn-first and
        # language-specific (a stub's shape differs per stack), so — like the perf
        # *meters* — a project opts in by adding its own step here, e.g.:
        #   ("no-stubs", (), [sys.executable, "scripts/check_stubs.py"], {"G3"}, "product"),
        # (add --strict to make found stubs fail the gate). A non-Python stack
        # swaps or drops it. Left out of the default plan to keep the floor honest.
        # --- process checks: kit-owned, stdlib-only, identical everywhere -----
        ("traceability", (), trace_cmd, {"G2", "G3"}, "process"),
        # Doc navigability (process.md §3 "Reviewability"): broken intra-repo
        # links fail; orphans warn. Runs from G1 on (docs exist early). The
        # generated, gitignored trace report is dropped from the scanned set.
        (
            "doc-navigability",
            (),
            [
                sys.executable,
                "scripts/check_docs.py",
                "--ignore",
                "docs/test/report.md",
            ],
            {"G1", "G2", "G3"},
            "process",
        ),
        # Performance budgets (process.md §9): the kit-owned *comparator* (stdlib,
        # metric-agnostic) checks the project's measured perf-metrics.json against
        # the budgets registry + committed baseline. Tier-threaded so size-class
        # budgets gate at full and noisy runtime ones warn at release; absent
        # metrics/budgets skip. The *measurement* that emits perf-metrics.json is
        # a PRODUCT step you wire to your stack (see EDIT FOR YOUR STACK above).
        (
            "perf-budgets",
            (),
            [sys.executable, "scripts/check_perf.py", "--tier", tier],
            {"G3"},
            "process",
        ),
        # Authored runtime-flow diagrams (process.md §3 "Design-time runtime
        # flows"): required from G2 on, so reviewers verify behavior from the
        # diagrams, not from registry rows.
        (
            "design-flows",
            (),
            [sys.executable, "scripts/check_flows.py", "--no-placeholders"],
            {"G2", "G3"},
            "process",
        ),
        # Add `--doc AGENTS.md` / `--doc CLAUDE.md` to route the map there too, and
        # `--flow <entry>` to also check the generated high-level flow.
        (
            "arch-map",
            (),
            [
                sys.executable,
                "scripts/gen_arch_map.py",
                "--check",
                "--strict-parse",
                "--src",
                SRC,
                "--doc",
                "docs/architecture.md",
            ],
            {"G3"},
            "process",
        ),
    ]


GATES = ["G1", "G2", "G3", "all"]

# The machine-readable active gate (process.md §7). One line, e.g. "G1".
GATE_FILE = Path("docs/gate")


def resolve_gate(explicit):
    """The gate to run: an explicit --gate wins; else the docs/gate file (the
    project's recorded active gate); else 'all' (a repo without the file gets
    the full bar, never a silently weaker one)."""
    if explicit:
        return explicit
    if GATE_FILE.exists():
        val = GATE_FILE.read_text(encoding="utf-8").strip()
        if val not in GATES:
            sys.exit(
                "check: docs/gate contains {!r}; expected one of {}".format(
                    val, "|".join(GATES)
                )
            )
        return val
    return "all"


def run_step(name, requires, cmd, lenient):
    """Run one step. Returns (status, detail) where status in PASS/FAIL/SKIP."""
    missing = [m for m in requires if importlib.util.find_spec(m) is None]
    if missing:
        status = "SKIP" if lenient else "FAIL"
        return status, "module(s) {} not importable by {} — run scripts/setup".format(
            ", ".join(missing), sys.executable
        )
    start = time.time()
    print("\n=== {} : {} ===".format(name, " ".join(cmd)), flush=True)
    proc = subprocess.run(cmd)
    secs = time.time() - start
    if proc.returncode == 0:
        return "PASS", "{:.1f}s".format(secs)
    return "FAIL", "exit {} ({:.1f}s)".format(proc.returncode, secs)


def main():
    _utf8_console()
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument(
        "--gate",
        choices=GATES,
        default=None,
        help="gate to run (default: the active gate in docs/gate, else all)",
    )
    ap.add_argument("--tier", choices=list(TIERS), default="all")
    ap.add_argument("--coverage", type=int, default=COVERAGE_THRESHOLD)
    ap.add_argument(
        "--phase",
        default=None,
        help="delivery phase(s) in scope, e.g. v1 or v1,v2 — scopes the G3 "
        "Verified criterion to that phase (process.md §4 'Phased delivery')",
    )
    ap.add_argument(
        "--lenient",
        action="store_true",
        help="treat missing tools as SKIP (local dev only)",
    )
    ap.add_argument(
        "--list",
        action="store_true",
        help="print the plan (with [process]/[product] layer tags) and exit",
    )
    args = ap.parse_args()
    gate = resolve_gate(args.gate)

    plan = [
        s
        for s in steps(args.coverage, args.tier, gate, args.phase)
        if gate == "all" or gate in s[3]
    ]

    if args.list:
        print("Plan for gate {} (tier {}):".format(gate, args.tier))
        for name, _requires, cmd, gates, layer in plan:
            print(
                "  - {:16} [{:7}] [{}]  {}".format(
                    name, layer, ",".join(sorted(gates)), " ".join(cmd)
                )
            )
        return

    if not plan:
        print("No checks defined for gate {}.".format(gate))
        return

    results = []
    for name, requires, cmd, _gates, _layer in plan:
        status, detail = run_step(name, requires, cmd, args.lenient)
        results.append((name, status, detail))

    print("\n" + "=" * 56)
    print("Check summary (gate {}, tier {}):".format(gate, args.tier))
    for name, status, detail in results:
        print("  {:5} {:16} {}".format(status, name, detail))
    failed = [r for r in results if r[1] == "FAIL"]
    print("=" * 56)
    if failed:
        print("RESULT: FAIL ({} step(s) failed)".format(len(failed)))
        sys.exit(1)
    print("RESULT: PASS")


if __name__ == "__main__":
    main()
