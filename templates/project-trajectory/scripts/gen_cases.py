#!/usr/bin/env python3
"""Generate test-case combinations from a requirement's input dimensions.

Stack-agnostic kit, stdlib-only (Python 3.8+). A requirement is rarely satisfied
by one happy-path test: each variable input is a *dimension*, and defects hide at
the **boundaries** of each dimension and in the **interactions** between them.
Enumerating the full Cartesian product exercises every interaction but explodes
(d dimensions of k values = k**d cases) and gets untenable fast. This tool
derives the values worth testing per dimension and then combines them by a chosen
strategy so you exercise the extremes *in combination* without paying for the
full product.

Techniques it applies (see process.md "Dimensional test coverage"):
    - Boundary-value analysis: a `range[min..max]` contributes its **min and max**
      (plus any interior/nominal points you list) — the classic off-by-one /
      overflow catchers. Note: this tool combines over the *valid* input space;
      just-outside/invalid values assert rejection, so design those by hand as
      their own error-path TCs (see process.md "Dimensional coverage").
    - Equivalence partitioning: a `set{...}` contributes one representative per
      class; `bool` is `{true,false}`.
    - Combination strategy:
        full       — Cartesian product (every interaction; use when small or
                     high-risk-and-cheap).
        pairwise   — all-pairs: every value of every dimension is paired with
                     every value of every other dimension at least once. Catches
                     the large majority of interaction bugs at a fraction of the
                     cost. The sensible default for >=3 dimensions.
        boundaries — extreme corners + single-factor extreme sweeps (all-low,
                     all-high, and each dimension flipped to its other extreme).
                     Cheapest; for expensive (hardware / integration) tiers.

Spec grammar (one string; ';'-separated dimensions, optional '@strategy'):
    name=range[min..max]            -> values: min, max
    name=range[min..max|n1|n2]      -> values: min, max, n1, n2  (interior/nominal)
    name=set{a,b,c}                 -> values: a, b, c
    name=bool                       -> values: true, false
    @full | @pairwise | @boundaries -> strategy (else: pairwise if >2 dims, else full)

  This is the same grammar used in the SR registry's `Permutations` column, so you
  can lift a cell straight into --spec.

Usage:
    python scripts/gen_cases.py --spec "SPEC" [--strategy S] [--format table|params|csv]
                                [--id SR-003] [--tier Full]

Examples:
    python scripts/gen_cases.py --spec "size=range[0..2GiB]; field=set{plain,comma,quote,newline}; enc=set{utf8,utf16}"
    python scripts/gen_cases.py --spec "mode=set{Mirror,HashAddressed}; compress=bool; count=range[0..1e6]" --format params
"""

import argparse
import itertools
import re


def parse_spec(spec):
    """Return (dims, strategy_or_None). dims = list of (name, values, boundary_flags)."""
    strategy = None
    dims = []
    for seg in spec.split(";"):
        seg = seg.strip()
        if not seg:
            continue
        if seg.startswith("@"):
            strategy = seg[1:].strip().lower()
            continue
        if "=" not in seg:
            raise SystemExit("bad dimension (no '='): {!r}".format(seg))
        name, rhs = seg.split("=", 1)
        name, rhs = name.strip(), rhs.strip()
        m = re.match(r"range\[(.+?)\.\.(.+?)(?:\|(.+))?\]$", rhs)
        if m:
            lo, hi = m.group(1).strip(), m.group(2).strip()
            interior = [v.strip() for v in (m.group(3) or "").split("|") if v.strip()]
            values = [lo, hi] + interior
            # boundary = the two extremes; interior points are nominal (not boundary)
            flags = [True, True] + [False] * len(interior)
        elif rhs.startswith("set{") and rhs.endswith("}"):
            values = [v.strip() for v in rhs[4:-1].split(",") if v.strip()]
            flags = [False] * len(values)
        elif rhs == "bool":
            values, flags = ["true", "false"], [True, True]
        else:
            raise SystemExit("unrecognized dimension type: {!r}".format(seg))
        if not values:
            raise SystemExit("dimension {!r} has no values".format(name))
        dims.append((name, values, flags))
    if not dims:
        raise SystemExit("no dimensions parsed from spec")
    return dims, strategy


def pair_key(a, va, b, vb):
    return (a, va, b, vb) if a < b else (b, vb, a, va)


def all_pairs(values):
    """Greedy all-pairs (pairwise) cover over a list of per-dimension value lists.

    Guarantees every (dim_i=value, dim_j=value) pair appears in >=1 case. Returns
    a list of value-tuples. Deterministic.
    """
    n = len(values)
    if n == 1:
        return [(v,) for v in values[0]]
    uncovered = set()
    for i in range(n):
        for j in range(i + 1, n):
            for vi in values[i]:
                for vj in values[j]:
                    uncovered.add(pair_key(i, vi, j, vj))

    tests = []
    while uncovered:
        # Seed the new case with an arbitrary still-uncovered pair (guarantees
        # progress), then greedily fill the remaining dimensions to cover the
        # most still-uncovered pairs.
        si, svi, sj, svj = sorted(uncovered)[0]
        test = [None] * n
        test[si], test[sj] = svi, svj
        for k in range(n):
            if test[k] is not None:
                continue
            best_v, best_cov = values[k][0], -1
            for v in values[k]:
                cov = sum(
                    1
                    for m in range(n)
                    if test[m] is not None
                    and m != k
                    and pair_key(k, v, m, test[m]) in uncovered
                )
                if cov > best_cov:
                    best_v, best_cov = v, cov
            test[k] = best_v
        for a in range(n):
            for b in range(a + 1, n):
                uncovered.discard(pair_key(a, test[a], b, test[b]))
        tests.append(tuple(test))
    return tests


def boundary_corners(dims):
    """All-low, all-high, and each dimension flipped to its other extreme.

    Low/high are the first/last *boundary-flagged* values (the extremes), falling
    back to the first/last listed value. Localizes which dimension breaks."""
    lows, highs = [], []
    for _name, values, flags in dims:
        bvals = [v for v, f in zip(values, flags) if f] or values
        lows.append(bvals[0])
        highs.append(bvals[-1])
    cases = [tuple(lows), tuple(highs)]
    for i in range(len(dims)):
        flipped = list(lows)
        flipped[i] = highs[i]
        cases.append(tuple(flipped))
    # de-dup, preserve order
    seen, out = set(), []
    for c in cases:
        if c not in seen:
            seen.add(c)
            out.append(c)
    return out


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--spec", required=True, help="dimensional spec (see grammar)")
    ap.add_argument(
        "--strategy",
        choices=["full", "pairwise", "boundaries"],
        default=None,
        help="override the spec's @strategy / the default",
    )
    ap.add_argument("--format", choices=["table", "params", "csv"], default="table")
    ap.add_argument(
        "--id", default="", help="requirement id to label rows (e.g. SR-003)"
    )
    ap.add_argument("--tier", default="Full", help="Tier to stamp on emitted TC rows")
    args = ap.parse_args()

    dims, spec_strategy = parse_spec(args.spec)
    names = [d[0] for d in dims]
    values = [d[1] for d in dims]
    strategy = (
        args.strategy or spec_strategy or ("pairwise" if len(dims) > 2 else "full")
    )

    full_count = 1
    for v in values:
        full_count *= len(v)

    if strategy == "full":
        cases = list(itertools.product(*values))
    elif strategy == "pairwise":
        cases = all_pairs(values)
    elif strategy == "boundaries":
        cases = boundary_corners(dims)
    else:
        raise SystemExit("unknown strategy: {}".format(strategy))

    def param_str(case):
        return "; ".join("{}={}".format(n, v) for n, v in zip(names, case))

    # Dimensional analysis to stderr-ish header (kept on stdout for capture).
    print("# Dimensional analysis" + (" for {}".format(args.id) if args.id else ""))
    for name, vals, flags in dims:
        marked = ", ".join(("*{}*".format(v) if f else v) for v, f in zip(vals, flags))
        print(
            "  - {} ({} values): {}".format(name, len(vals), marked)
            + ("   (* = boundary)" if any(flags) else "")
        )
    print(
        "  strategy: {}   cases: {}  (full product = {}; {:.0f}% reduction)".format(
            strategy,
            len(cases),
            full_count,
            100 * (1 - len(cases) / full_count) if full_count else 0,
        )
    )
    print()

    if args.format == "params":
        for c in cases:
            print(param_str(c))
    elif args.format == "csv":
        # TC rows ready to paste (fill TC-ID / Verifies / Expected).
        print("TC-ID,Verifies,Level,Method,Tier,Parameters,Expected,Automated,Status")
        for c in cases:
            print(
                'TC-xxx,{},Unit,{} combination,{},"{}",'
                '"Satisfies {} AcceptanceCriteria",Yes,Draft'.format(
                    args.id or "SR-xxx",
                    strategy,
                    args.tier,
                    param_str(c),
                    args.id or "SR-xxx",
                )
            )
    else:
        print("| # | " + " | ".join(names) + " |")
        print("|---|" + "|".join("---" for _ in names) + "|")
        for i, c in enumerate(cases, 1):
            print("| {} | {} |".format(i, " | ".join(str(x) for x in c)))


if __name__ == "__main__":
    main()
