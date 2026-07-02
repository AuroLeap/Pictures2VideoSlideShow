#!/usr/bin/env python3
"""Performance budget & regression comparator: track the numbers, alert on drift.

Captured budgets (`performance-budgets.csv`, PB-###, process.md §9) are inert
without a check that compares the *measured* numbers against them over time. This
is that check — and it splits cleanly along the process/product line (process.md
§7): **measuring** a metric is *product* work the project wires (`/usr/bin/time`,
`tracemalloc`, `nvidia-smi`, a size command, `pytest-benchmark` / `hyperfine`);
**comparing** is *process* work the kit owns — this script, stdlib-only and
metric-agnostic (arithmetic over JSON). The kit owns the comparator; the project
owns the meters.

Two distinct questions, per metric:
    - **absolute** — "worse than the budget?" (measured vs `Budget`, per `Direction`)
    - **regression** — "suddenly much worse?" (measured vs a committed baseline,
      outside the per-metric `Tolerance` band)

Inputs (all optional — a missing one degrades to a skip, never a false failure):
    - `--budgets`  docs/requirements/performance-budgets.csv  (tracked source of truth)
    - `--metrics`  docs/test/perf-metrics.json  (product-emitted: {"PB-001": 480, ...})
    - `--baseline` docs/test/perf-baseline.json (committed golden, same shape)

Output:
    - `--report`   docs/test/perf-report.md  (current vs baseline vs budget +
      deltas) — a gitignored composite artifact (process.md §3), regenerated each
      run; review the budgets CSV + baseline diff, not this.

Exit code is nonzero **only** on a hard-gated breach — a `Gate=fail` row that
breaches its absolute budget or regresses beyond tolerance, within the run tier.
`Gate=warn` rows (the default for noisy runtime metrics) only warn. A metric with
no measurement this run is skipped. This warn-first, start-with-deterministic-
metrics stance is the honest-gate rule (§4): a number that can't be a reliable
`Test` gate is tracked or `Demonstration`, never faked into a binary gate.

**Baseline-as-golden protocol:** accepting a regression means committing a new
baseline *in the same PR*, so the number move is explicit and reviewed (same
discipline as the coverage threshold). `--update-baseline` rewrites
`perf-baseline.json` from the current metrics for exactly that purpose.

Usage:
    python scripts/check_perf.py [--tier smoke|full|release|all]
                                 [--root DIR] [--docs DIR]
                                 [--budgets P] [--metrics P] [--baseline P]
                                 [--report P] [--update-baseline]

--root/--docs are the path flags shared with trace.py and check_docs.py: the
four artifact paths default under <root>/docs unless given explicitly.
"""

import argparse
import csv
import json
import sys
from pathlib import Path


def _utf8_console():
    """Emit UTF-8 to stdout/stderr whatever the OS console codepage is. Kit
    scripts print non-ASCII (an em-dash WARNING, `§`/unit glyphs) that a legacy
    Windows cp1252 console raises UnicodeEncodeError on — wedging the run, not
    just mojibaking. Python 3.7+ streams expose `.reconfigure`; guard the rest."""
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8")
        except (AttributeError, ValueError):
            pass


# Cumulative tiers (process.md §4): a row is in scope at run tier R when its own
# tier is <= R. A blank row tier defaults to Full — evaluated pre-merge and at
# release, never silently dropped, but not gating the cheap smoke run.
TIER_ORDER = {"smoke": 0, "full": 1, "release": 2, "all": 2}
DEFAULT_ROW_TIER = "full"


def load_budgets(path):
    """Real PB rows from the registry (the -000 example row is ignored, like
    everywhere else; the registry is optional, so a missing file = no budgets)."""
    if not path.exists():
        return []
    with path.open(newline="", encoding="utf-8-sig") as f:
        rows = list(csv.DictReader(f))
    return [r for r in rows if r.get("PB-ID") and not r["PB-ID"].endswith("-000")]


def load_metrics(path):
    """Read a {PB-ID: number} JSON map. A value may also be {"value": number}.
    Returns None when the file is absent (the 'not measured this run' signal)."""
    if not path.exists():
        return None
    data = json.loads(path.read_text(encoding="utf-8"))
    out = {}
    for k, v in data.items():
        if isinstance(v, dict):
            v = v.get("value")
        if isinstance(v, bool) or not isinstance(v, (int, float)):
            continue  # ignore non-numeric / malformed entries
        out[k] = float(v)
    return out


def to_number(s):
    try:
        return float(str(s).strip())
    except (TypeError, ValueError):
        return None


def parse_tolerance(s):
    """('pct', 0.10) for "10%", ('abs', 5.0) for "5", or None when blank."""
    s = (s or "").strip()
    if not s:
        return None
    if s.endswith("%"):
        pct = to_number(s[:-1])
        return ("pct", pct / 100.0) if pct is not None else None
    val = to_number(s)
    return ("abs", val) if val is not None else None


def direction_of(row):
    d = (row.get("Direction") or "").strip().lower()
    return d if d in ("lower-better", "higher-better") else "lower-better"


def in_tier(row_tier, run_tier):
    rt = (row_tier or DEFAULT_ROW_TIER).strip().lower()
    return TIER_ORDER.get(rt, 1) <= TIER_ORDER.get(run_tier.strip().lower(), 2)


def absolute_breach(measured, budget, direction):
    """True when `measured` is worse than `budget` (None budget = no abs check)."""
    if budget is None:
        return False
    return measured > budget if direction == "lower-better" else measured < budget


def regression_limit(baseline, tol, direction):
    """The worst value still tolerated relative to `baseline`, or None when a
    regression check can't be computed (no baseline or no tolerance)."""
    if baseline is None or tol is None:
        return None
    kind, amount = tol
    delta = baseline * amount if kind == "pct" else amount
    return baseline + delta if direction == "lower-better" else baseline - delta


def regression_breach(measured, limit, direction):
    if limit is None:
        return False
    return measured > limit if direction == "lower-better" else measured < limit


def evaluate(budgets, metrics, baselines, run_tier):
    """Compare each in-tier budget row against its measured value. Returns a list
    of per-row result dicts (status in OK / WARN / FAIL / SKIP)."""
    results = []
    for row in budgets:
        pid = row["PB-ID"]
        if not in_tier(row.get("Tier"), run_tier):
            continue
        direction = direction_of(row)
        budget = to_number(row.get("Budget"))
        tol = parse_tolerance(row.get("Tolerance"))
        gate = (row.get("Gate") or "warn").strip().lower()
        measured = metrics.get(pid) if metrics else None
        baseline = baselines.get(pid) if baselines else None

        res = {
            "pid": pid,
            "metric": (row.get("Metric") or "").strip(),
            "unit": (row.get("Unit") or "").strip(),
            "direction": direction,
            "budget": budget,
            "gate": gate,
            "tier": (row.get("Tier") or DEFAULT_ROW_TIER).strip(),
            "measured": measured,
            "baseline": baseline,
            "abs_breach": False,
            "reg_breach": False,
        }
        if measured is None:
            res["status"] = "SKIP"
            results.append(res)
            continue
        res["abs_breach"] = absolute_breach(measured, budget, direction)
        limit = regression_limit(baseline, tol, direction)
        res["reg_breach"] = regression_breach(measured, limit, direction)
        if res["abs_breach"] or res["reg_breach"]:
            res["status"] = "FAIL" if gate == "fail" else "WARN"
        else:
            res["status"] = "OK"
        results.append(res)
    return results


def fmt(n):
    if n is None:
        return "—"
    return str(int(n)) if float(n).is_integer() else f"{n:g}"


def delta_str(measured, baseline):
    if measured is None or baseline is None:
        return "—"
    diff = measured - baseline
    sign = "+" if diff >= 0 else "−"
    if baseline:
        return f"{sign}{fmt(abs(diff))} ({sign}{abs(diff) / baseline * 100:.1f}%)"
    return f"{sign}{fmt(abs(diff))}"


def reason(res):
    bits = []
    if res["abs_breach"]:
        worse = "exceeds" if res["direction"] == "lower-better" else "below"
        bits.append(f"{worse} budget {fmt(res['budget'])}{res['unit']}")
    if res["reg_breach"]:
        bits.append(f"regressed vs baseline {fmt(res['baseline'])}{res['unit']}")
    return "; ".join(bits)


def render_report(results, run_tier):
    lines = [
        "# Performance budget report",
        "",
        "_Generated by `scripts/check_perf.py` (tier `{}`). A gitignored composite "
        "(process.md §3): review the budgets CSV + baseline diff, not this file._".format(
            run_tier
        ),
        "",
        "| Status | PB-ID | Metric | Measured | Baseline | Δ vs base | Budget | Dir | Gate |",
        "|---|---|---|---|---|---|---|---|---|",
    ]
    for r in results:
        unit = r["unit"]
        lines.append(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |".format(
                r["status"],
                r["pid"],
                r["metric"],
                f"{fmt(r['measured'])}{unit}" if r["measured"] is not None else "—",
                f"{fmt(r['baseline'])}{unit}" if r["baseline"] is not None else "—",
                delta_str(r["measured"], r["baseline"]),
                f"{fmt(r['budget'])}{unit}" if r["budget"] is not None else "—",
                "↓" if r["direction"] == "lower-better" else "↑",
                r["gate"],
            )
        )
    breaches = [r for r in results if r["status"] in ("FAIL", "WARN")]
    if breaches:
        lines += ["", "## Findings", ""]
        for r in breaches:
            lines.append(f"- **{r['status']}** {r['pid']} — {reason(r)}")
    lines.append("")
    return "\n".join(lines)


def update_baseline(baseline_path, metrics):
    """Merge the current metrics into the committed baseline and write it back
    (sorted, pretty). Returns the list of (pid, old, new) that changed."""
    existing = {}
    if baseline_path.exists():
        existing = json.loads(baseline_path.read_text(encoding="utf-8"))
    changes = []
    for pid, val in metrics.items():
        old = existing.get(pid)
        if old != val:
            changes.append((pid, old, val))
        existing[pid] = val
    baseline_path.parent.mkdir(parents=True, exist_ok=True)
    baseline_path.write_text(
        json.dumps(existing, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return changes


def main():
    _utf8_console()
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--tier", default="all", choices=list(TIER_ORDER))
    # --root/--docs are the uniform path flags shared with trace.py and
    # check_docs.py: pass one --root (or --docs) and the four artifact paths
    # below default under it. Each explicit path flag still wins when given.
    ap.add_argument("--root", default=".", help="repo root (default: .)")
    ap.add_argument(
        "--docs", default=None, help="docs directory (default: <root>/docs)"
    )
    ap.add_argument("--budgets", default=None)
    ap.add_argument("--metrics", default=None)
    ap.add_argument("--baseline", default=None)
    ap.add_argument("--report", default=None)
    ap.add_argument(
        "--update-baseline",
        action="store_true",
        help="rewrite perf-baseline.json from the current metrics (the reviewed, "
        "in-PR way to accept a regression) and exit",
    )
    args = ap.parse_args()

    docs = Path(args.docs) if args.docs else Path(args.root) / "docs"
    budgets_path = args.budgets or docs / "requirements" / "performance-budgets.csv"
    metrics_path = args.metrics or docs / "test" / "perf-metrics.json"
    baseline_path = args.baseline or docs / "test" / "perf-baseline.json"
    report_path = args.report or docs / "test" / "perf-report.md"

    metrics = load_metrics(Path(metrics_path))

    if args.update_baseline:
        if not metrics:
            print(f"check_perf: cannot update baseline - no metrics at {metrics_path}")
            sys.exit(1)
        changes = update_baseline(Path(baseline_path), metrics)
        print(
            f"check_perf: baseline updated ({len(changes)} change(s)) -> {baseline_path}"
        )
        for pid, old, new in changes:
            print(
                f"    {pid}: {fmt(old) if old is not None else '(new)'} -> {fmt(new)}"
            )
        return

    budgets = load_budgets(Path(budgets_path))
    if not budgets:
        print("check_perf: OK - no performance budgets to compare (process.md §9)")
        return
    if metrics is None:
        print(
            f"check_perf: SKIP - {len(budgets)} budget(s) but no metrics at "
            f"{metrics_path} (wire the product measurement step that emits it)"
        )
        return

    baselines = load_metrics(Path(baseline_path))
    results = evaluate(budgets, metrics, baselines, args.tier)

    report = Path(report_path)
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(render_report(results, args.tier), encoding="utf-8")

    for r in results:
        if r["status"] in ("FAIL", "WARN"):
            print(f"check_perf: {r['status']} - {r['pid']} {r['metric']}: {reason(r)}")
    fails = [r for r in results if r["status"] == "FAIL"]
    warns = [r for r in results if r["status"] == "WARN"]
    skips = [r for r in results if r["status"] == "SKIP"]
    summary = (
        f"{len(results)} in-tier budget(s): {len(fails)} fail, {len(warns)} warn, "
        f"{len(skips)} skip(no metric) -> {report_path}"
    )
    if fails:
        print(f"check_perf: FAIL - {summary}")
        sys.exit(1)
    print(f"check_perf: OK - {summary}")


if __name__ == "__main__":
    main()
