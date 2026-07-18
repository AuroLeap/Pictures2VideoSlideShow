#!/usr/bin/env python3
"""No-stub / substance detector: flag implementations that only *exist* (process.md §4 G3).

The kit's gates verify *traceability + coverage + tests pass*; none of them, on
its own, asserts that an implementation has **substance**. TDD (G3) mitigates
this — a red-first test should fail against a stub — but coverage can be satisfied
by a test that exercises a stub's trivial path, and Demonstration / Manual /
Analysis SRs have no automated test to fail. The G3 *no-stub* exit criterion
(process.md §4) closes that gap, classified **Inspection/Analysis** — human/LLM
judgment, never a machine verdict. This script is the optional, **Python-reference
tripwire** for that criterion: it lists public symbols whose bodies do nothing.

It is deliberately **product-layer, not a process check** (process.md §7): a stub's
shape is language-specific (`pass`/`...` here; `todo!()` in Rust; `throw new
NotImplementedException()` in C#), so unlike `gen_arch_map.py` — which every stack
re-implements into the *same* marker block — a stub detector has no shared
artifact. So it ships like the perf *meters*: an opt-in example a Python project
wires (or a non-Python stack swaps/drops), **outside the required process floor**.

It is **warn-first** by design (exit 0 even when stubs are found): the real call
is the human/LLM inspection the G3 criterion names, and a legitimately tiny pure
function must not be mistaken for an unfinished one. Pass `--strict` to make found
stubs exit nonzero — for a project that has chosen to enforce the criterion.

A "stub" is a public (no leading `_`) function or method whose body, after an
optional docstring, is exactly one of:

    pass
    ...                       (a bare Ellipsis)
    raise NotImplementedError (bare or called)
    return None               (or a bare `return`)

…or *only* a docstring (which silently returns None). `@abstractmethod` and
`@overload` symbols are skipped — their empty bodies are the point. Private names,
private classes, and nested/inner functions are out of scope. A function that
returns a real value (`return x`, `return x * 2`) is never a stub, so a deliberately
tiny pure core is not over-flagged.

Output: a gitignored composite report (process.md §3) of the candidates, plus a
WARN line per finding on stdout.

Usage:
    python scripts/check_stubs.py [--src src] [--report docs/test/stub-report.md]
                                  [--exclude GLOB ...] [--strict]
"""

import argparse
import ast
import sys
from pathlib import Path

# Decorators whose trivial body is intentional, never a stub: an abstract method
# or a typing overload is *supposed* to have an empty body. Dotted forms
# (`abc.abstractmethod`, `typing.overload`) match on the final attribute.
SKIP_DECORATORS = {"abstractmethod", "abstractproperty", "overload"}


def _name_of(node):
    """Final identifier of a Name/Attribute/Call expression (for decorators and
    raised exceptions): `overload` -> "overload", `abc.abstractmethod` ->
    "abstractmethod", `NotImplementedError(...)` -> "NotImplementedError"."""
    if isinstance(node, ast.Call):
        node = node.func
    if isinstance(node, ast.Attribute):
        return node.attr
    if isinstance(node, ast.Name):
        return node.id
    return None


def _is_docstring(stmt):
    return (
        isinstance(stmt, ast.Expr)
        and isinstance(stmt.value, ast.Constant)
        and isinstance(stmt.value.value, str)
    )


def stub_kind(func):
    """Return the stub shape of a FunctionDef/AsyncFunctionDef as a short label
    (e.g. "pass", "...", "raise NotImplementedError", "return None",
    "docstring-only"), or None when the body does real work."""
    body = func.body
    if body and _is_docstring(body[0]):
        body = body[1:]
    if not body:
        return "docstring-only"  # a docstring-only body returns None
    if len(body) != 1:
        return None
    s = body[0]
    if isinstance(s, ast.Pass):
        return "pass"
    if isinstance(s, ast.Expr) and isinstance(s.value, ast.Constant):
        if s.value.value is Ellipsis:
            return "..."
        return None
    if isinstance(s, ast.Return):
        if s.value is None or (
            isinstance(s.value, ast.Constant) and s.value.value is None
        ):
            return "return None"
        return None
    if isinstance(s, ast.Raise) and _name_of(s.exc) == "NotImplementedError":
        return "raise NotImplementedError"
    return None


def _skip_decorated(func):
    return any(_name_of(d) in SKIP_DECORATORS for d in func.decorator_list)


def _is_public(name):
    return not name.startswith("_")


def find_stubs(tree):
    """Public module-level functions and public methods (one class level deep)
    with a trivial body. Returns dicts {name, lineno, kind}; `name` is the bare
    function name or `Class.method`."""
    found = []

    def consider(func, prefix):
        if not _is_public(func.name) or _skip_decorated(func):
            return
        kind = stub_kind(func)
        if kind:
            found.append(
                {"name": prefix + func.name, "lineno": func.lineno, "kind": kind}
            )

    defs = (ast.FunctionDef, ast.AsyncFunctionDef)
    for node in tree.body:
        if isinstance(node, defs):
            consider(node, "")
        elif isinstance(node, ast.ClassDef) and _is_public(node.name):
            for sub in node.body:
                if isinstance(sub, defs):
                    consider(sub, node.name + ".")
    return found


def scan_source(text):
    """Parse Python source and return its stub findings (importable unit). Raises
    SyntaxError on unparseable input — callers warn-skip and continue."""
    return find_stubs(ast.parse(text))


def iter_py_files(src, root, exclude):
    """Every `*.py` under `src`, skipping repo-relative globs in `exclude`."""
    out = []
    for p in sorted(src.rglob("*.py")):
        relp = p.relative_to(root).as_posix() if _within(p, root) else p.as_posix()
        if any(Path(relp).match(g) for g in exclude):
            continue
        out.append(p)
    return out


def _within(path, root):
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def rel(path, root):
    return path.relative_to(root).as_posix() if _within(path, root) else path.as_posix()


def render_report(findings, n_files):
    lines = [
        "# Stub / substance report",
        "",
        "_Generated by `scripts/check_stubs.py` — an **optional, warn-first**, "
        "product-layer tripwire for the G3 no-stub criterion (process.md §4). A "
        "gitignored composite (process.md §3); the real call is the Inspection the "
        "criterion names, not this list._",
        "",
    ]
    if not findings:
        lines += [
            "No trivial-bodied public symbols found across {} file(s).".format(n_files),
            "",
        ]
        return "\n".join(lines)
    lines += [
        "{} candidate(s) across {} file(s):".format(len(findings), n_files),
        "",
        "| File:line | Symbol | Body |",
        "|---|---|---|",
    ]
    for f in findings:
        lines.append(
            "| {}:{} | `{}` | `{}` |".format(
                f["file"], f["lineno"], f["name"], f["kind"]
            )
        )
    lines.append("")
    return "\n".join(lines)


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--src", default="src", help="source root to scan (default: src)")
    ap.add_argument("--root", default=".", help="repo root for display/exclude paths")
    ap.add_argument("--report", default="docs/test/stub-report.md")
    ap.add_argument(
        "--exclude",
        action="append",
        default=[],
        metavar="GLOB",
        help="repo-relative glob of source files to skip (repeatable)",
    )
    ap.add_argument(
        "--strict",
        action="store_true",
        help="exit nonzero when stubs are found (default: warn-first, exit 0)",
    )
    args = ap.parse_args()

    root = Path(args.root).resolve()
    src = Path(args.src)
    if not src.is_absolute():
        src = root / src
    if not src.is_dir():
        print("check_stubs: OK - no source directory at {}".format(args.src))
        return

    files = iter_py_files(src, root, args.exclude)
    findings = []
    for p in files:
        try:
            stubs = scan_source(p.read_text(encoding="utf-8"))
        except SyntaxError as e:
            print(
                "check_stubs: WARN - skipped unparseable {} ({})".format(
                    rel(p, root), e
                )
            )
            continue
        for s in stubs:
            s["file"] = rel(p, root)
            findings.append(s)

    report = Path(args.report)
    if not report.is_absolute():
        report = root / report
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(render_report(findings, len(files)), encoding="utf-8")

    for f in findings:
        print(
            "check_stubs: WARN - {}:{} {} is a stub ({})".format(
                f["file"], f["lineno"], f["name"], f["kind"]
            )
        )

    if not findings:
        print(
            "check_stubs: OK - no stub bodies among the public symbols in {} "
            "file(s) -> {}".format(len(files), args.report)
        )
        return
    summary = "{} possible stub(s) across {} file(s) -> {}".format(
        len(findings), len(files), args.report
    )
    if args.strict:
        print("check_stubs: FAIL - {}".format(summary))
        sys.exit(1)
    # Warn-first: surfaced for the G3 Inspection, but never blocks on its own.
    print("check_stubs: WARN - {} (advisory; --strict to gate)".format(summary))


if __name__ == "__main__":
    main()
