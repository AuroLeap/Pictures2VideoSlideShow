#!/usr/bin/env python3
"""Generate the module/function map for `architecture.md` from the source tree.

Stack-agnostic kit, **Python reference implementation** (stdlib only — uses
`ast`, no pip installs). It keeps `architecture.md` honest: the hand-written
overview stays, and everything between the GENERATED markers is regenerated
here so it cannot drift from the code. Swap this script for an equivalent in
your stack (e.g. `tsc`/ts-morph for TypeScript, `go doc` for Go) — the contract
is only the marker block it fills.

What it emits, per module (one section each):
    - the module's one-line **summary** (its module docstring) — so an agent
      grasps each file's job without opening it;
    - **internal coupling**: which other in-tree modules it imports (best-effort)
      — makes layering/dependency invariants visible (e.g. "Common must not
      import Engine") and tells an agent the blast radius of a change;
    - each public symbol's **signature**, one-line docstring summary, and any
      `Implements: SR-###, LLR-###` back-links found near it.

This is the **AI/human code map**: a current, greppable index of where each
capability lives, what it depends on, and which requirement it implements — so an
agent edits the right place instead of re-deriving the layout. It is harvested
from your docstrings/headers, which is one more reason to comment for humans
(see AGENTS.template.md "Comment for humans — and the map").

Routing: `--doc` is repeatable, so the same generated block can be spliced into
`docs/architecture.md` AND the agent's primary file (`AGENTS.md` / `CLAUDE.md`) —
wherever the marker pair lives. Embed it where agents actually read.

Program flow (`--flow ENTRY`): emit the **ordered internal calls** an entry/
orchestrator function makes, each with the callee's one-line summary, into a
GENERATED FLOW block. This makes the high-level flow readable at a glance AND
acts as a tripwire: a thin orchestrator yields a clean numbered flow; one that
inlines logic instead of delegating shows up as a short, uninformative list.
(Control flow — loops/branches — is not represented; keep the overview prose for
that.)

Dependency diagram: the same internal imports the map lists, rendered as a
Mermaid `graph LR` and spliced into the DEPENDENCY DIAGRAM markers wherever a
--doc has them (architecture.md ships with the pair). Output is plain text —
GitHub/GitLab and the VS Code Markdown preview render mermaid fences natively,
so the kit needs no diagram toolchain — and layering violations (e.g. an arrow
from `common` into `engine`) are visible at a glance.

A syntax-broken module is rendered as a `PARSE ERROR` summary rather than
crashing the run (so the rest of the map still generates). That keeps the error
*visible*, but `--check` alone would still pass once the PARSE ERROR text is
written; pass `--strict-parse` to also *fail* on any unparseable module — for a
non-Python stack where this map is the only parse signal, or to belt-and-braces
the lint/test steps.

Usage:
    python scripts/gen_arch_map.py [--src SRC ...] [--doc FILE ...] [--flow ENTRY]
                                   [--check] [--strict-parse]

    --src           One or more source roots to scan (default: src). Repeatable.
    --doc           File(s) to update in place (default: docs/architecture.md).
                    Repeatable — each must contain the MODULE MAP marker pair.
    --flow          Entry function (e.g. `run` or `module:run`) whose call
                    sequence is spliced into the FLOW markers of any --doc.
    --check         Do not write; exit 1 if any target is out of date (CI/harness).
    --strict-parse  Exit 1 if any scanned module fails to parse (independent of
                    --check staleness).

Marker pairs (the templates ship with them):
    <!-- BEGIN GENERATED MODULE MAP -->  ... <!-- END GENERATED MODULE MAP -->   (required per --doc)
    <!-- BEGIN GENERATED FLOW -->        ... <!-- END GENERATED FLOW -->          (optional; used by --flow)
    <!-- BEGIN GENERATED DEPENDENCY DIAGRAM --> ... <!-- END GENERATED DEPENDENCY DIAGRAM -->  (optional)
"""

import argparse
import ast
import re
import sys
from pathlib import Path


def _utf8_console():
    """Emit UTF-8 to stdout/stderr whatever the OS console codepage is. Kit
    scripts print non-ASCII (an em-dash WARNING, `§` refs) that a legacy Windows
    cp1252 console raises UnicodeEncodeError on — wedging the run, not just
    mojibaking. Python 3.7+ streams expose `.reconfigure`; guard for the rest."""
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8")
        except (AttributeError, ValueError):
            pass


BEGIN = "<!-- BEGIN GENERATED MODULE MAP -->"
END = "<!-- END GENERATED MODULE MAP -->"
BEGIN_FLOW = "<!-- BEGIN GENERATED FLOW -->"
END_FLOW = "<!-- END GENERATED FLOW -->"
BEGIN_DIAGRAM = "<!-- BEGIN GENERATED DEPENDENCY DIAGRAM -->"
END_DIAGRAM = "<!-- END GENERATED DEPENDENCY DIAGRAM -->"
IMPLEMENTS_RE = re.compile(r"\b(?:SR|LLR|SN|TC)-\d+\b")


def first_line(text):
    """First non-empty line of a docstring, trimmed."""
    for line in (text or "").strip().splitlines():
        line = line.strip()
        if line:
            return line
    return ""


def signature(node):
    """Render a function/method signature from its AST args (names only)."""
    a = node.args
    parts = [p.arg for p in (a.posonlyargs + a.args)]
    if a.vararg:
        parts.append("*" + a.vararg.arg)
    if a.kwonlyargs:
        if not a.vararg:
            parts.append("*")
        parts += [p.arg for p in a.kwonlyargs]
    if a.kwarg:
        parts.append("**" + a.kwarg.arg)
    return "({})".format(", ".join(parts))


def implements(node, source_lines):
    """Collect requirement ids annotated near a symbol (docstring + the few
    comment lines just above its definition)."""
    ids = set()
    doc = ast.get_docstring(node) or ""
    ids.update(IMPLEMENTS_RE.findall(doc))
    start = node.lineno - 1  # 0-based line of the def
    for i in range(max(0, start - 4), start):
        line = source_lines[i]
        if "Implements" in line or line.lstrip().startswith("#"):
            ids.update(IMPLEMENTS_RE.findall(line))
    return sorted(ids)


def internal_imports(tree, internal_names):
    """In-tree modules this file imports (best-effort: relative imports, or an
    absolute import whose first segment names a scanned module/package).

    `from pkg import mod` is recorded as `pkg.mod` when `mod` names a scanned
    module — the real dependency is the submodule, not the package — except
    when the imported name shadows the module it comes from (`from .util
    import util`), where the module itself is the right target."""
    found = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.ImportFrom):
            if node.level or (
                node.module and node.module.split(".")[0] in internal_names
            ):
                prefix = "." * node.level + (node.module or "")
                last = (node.module or "").split(".")[-1]
                submodules = [
                    a.name
                    for a in node.names
                    if a.name in internal_names and a.name != last
                ]
                if submodules:
                    sep = "." if node.module else ""
                    for sub in submodules:
                        found.add(prefix + sep + sub)
                else:
                    found.add(prefix)
        elif isinstance(node, ast.Import):
            for alias in node.names:
                if alias.name.split(".")[0] in internal_names:
                    found.add(alias.name)
    return sorted(found)


def scan_module(path, root, internal_names):
    """Return (rel_module, summary, imports, rows) for one .py file."""
    text = path.read_text(encoding="utf-8")
    source_lines = text.splitlines()
    rel = path.relative_to(root).with_suffix("").as_posix().replace("/__init__", "")
    try:
        tree = ast.parse(text)
    except SyntaxError as exc:  # surface, don't crash the whole run
        return rel, "PARSE ERROR: {}".format(exc), [], []
    summary = first_line(ast.get_docstring(tree))
    imports = internal_imports(tree, internal_names)
    rows = []
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            if node.name.startswith("_"):
                continue
            rows.append(
                (
                    node.name,
                    signature(node),
                    first_line(ast.get_docstring(node)),
                    implements(node, source_lines),
                )
            )
        elif isinstance(node, ast.ClassDef):
            if node.name.startswith("_"):
                continue
            methods = [
                n.name
                for n in node.body
                if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))
                and not n.name.startswith("_")
            ]
            rows.append(
                (
                    node.name + " (class)",
                    "",
                    first_line(ast.get_docstring(node)),
                    implements(node, source_lines),
                )
            )
            if methods:
                rows.append(("  methods", "", " · ".join(methods), []))
    return rel, summary, imports, rows


def _module_files(src_roots):
    """Yield (path, root_parent) for every scanned .py file, with the set of
    internal module/package names (for coupling detection)."""
    files = []
    names = set()
    for root in src_roots:
        root = Path(root)
        if not root.exists():
            continue
        for path in sorted(root.rglob("*.py")):
            if any(part.startswith((".", "__pycache__")) for part in path.parts):
                continue
            files.append((path, root.parent if root.name else root))
            names.add(path.stem)
            for part in path.relative_to(root).parts[:-1]:
                names.add(part)
    return files, names


def build_map(src_roots):
    files, internal_names = _module_files(src_roots)
    note = (
        "_Generated by `scripts/gen_arch_map.py` from the source tree (AST). "
        "Do not edit by hand; run the check harness to refresh. Summaries and "
        "`Implements:` come from your docstrings/comments._"
    )
    if not files:
        return note + "\n\n_(no source scanned)_"
    sections = [note]
    for path, root_parent in files:
        rel, summary, imports, rows = scan_module(path, root_parent, internal_names)
        if not (summary or imports or rows):
            continue  # skip empty modules (e.g. bare __init__.py) — no noise
        sections.append("\n### `{}`".format(rel))
        if summary:
            sections.append("_{}_".format(summary.replace("|", "\\|")))
        if imports:
            sections.append(
                "Imports (internal): {}".format(
                    ", ".join("`{}`".format(i) for i in imports)
                )
            )
        if rows:
            sections.append("\n| Public item | Summary | Implements |\n|---|---|---|")
            for name, sig, summ, ids in rows:
                sections.append(
                    "| `{}{}` | {} | {} |".format(
                        name,
                        sig,
                        summ.replace("|", "\\|"),
                        ", ".join(ids) if ids else "",
                    )
                )
        else:
            sections.append("_(no public items)_")
    return "\n".join(sections)


def collect_parse_errors(src_roots):
    """(rel, message) for every scanned module that fails to parse. Used by
    --strict-parse to fail the gate, rather than only surfacing the PARSE ERROR
    text in the map (which `--check` would treat as up to date)."""
    errs = []
    files, _ = _module_files(src_roots)
    for path, root_parent in files:
        try:
            ast.parse(path.read_text(encoding="utf-8"))
        except SyntaxError as exc:
            rel = (
                path.relative_to(root_parent)
                .with_suffix("")
                .as_posix()
                .replace("/__init__", "")
            )
            errs.append((rel, str(exc)))
    return errs


def _resolve_import(imp, importer_rel, known):
    """Map an import string from internal_imports() to a scanned module's rel
    path; None if it doesn't resolve. Falls back to progressively shorter
    prefixes (`pkg.sub.name` -> `pkg.sub` -> `pkg`) so a symbol that merely
    looks like a module still lands on its containing module. Best-effort,
    same caveat as the map."""
    if imp.startswith("."):
        level = len(imp) - len(imp.lstrip("."))
        # Drop the importer's own name, then one more segment per extra dot.
        base = importer_rel.split("/")[:-level]
        tail = imp.lstrip(".")
        parts = base + (tail.split(".") if tail else [])
    else:
        parts = imp.split(".")
    while parts:
        cand = "/".join(parts)
        for k in known:
            if k == cand or k.endswith("/" + cand):
                return k
        parts = parts[:-1]
    return None


def build_dependency_diagram(src_roots):
    """Mermaid `graph LR` of the internal-import graph — the imports the module
    map lists, as a picture. Plain text out; rendering is the viewer's job."""
    files, internal_names = _module_files(src_roots)
    note = (
        "_Generated by `scripts/gen_arch_map.py` from the source tree (AST): "
        "each arrow is an internal import. Do not edit by hand; run the check "
        "harness to refresh._"
    )
    if not files:
        return note + "\n\n_(no source scanned)_"
    mods = []
    for path, root_parent in files:
        rel, summary, imports, _rows = scan_module(path, root_parent, internal_names)
        mods.append((rel, summary, imports))
    known = [m[0] for m in mods]

    def node_id(rel):
        # Mermaid ids must avoid '/'-style separators and reserved words
        # (a module named `end` would otherwise break the graph) — prefix + sanitize.
        return "m_" + re.sub(r"\W", "_", rel)

    lines = [note, "", "```mermaid", "graph LR"]
    for rel, summary, _imports in mods:
        label = rel
        if summary:
            short = summary if len(summary) <= 48 else summary[:47] + "…"
            label = "{} — {}".format(rel, short)
        lines.append('    {}["{}"]'.format(node_id(rel), label.replace('"', "'")))
    edges = set()
    for rel, _summary, imports in mods:
        for imp in imports:
            target = _resolve_import(imp, rel, known)
            if target and target != rel:
                edges.add((node_id(rel), node_id(target)))
    for src_id, dst_id in sorted(edges):
        lines.append("    {} --> {}".format(src_id, dst_id))
    lines.append("```")
    return "\n".join(lines)


def splice_region(doc_text, begin, end, content, target, required):
    """Replace the text between begin/end markers. If the markers are absent:
    error when required, else leave the text untouched. Returns the new text."""
    if begin not in doc_text or end not in doc_text:
        if required:
            raise SystemExit(
                "{} is missing markers:\n  {}\n  {}".format(target, begin, end)
            )
        return doc_text
    # A duplicated marker would make the splice ambiguous (and silently eat the
    # text between the copies) — refuse rather than corrupt the doc.
    if doc_text.count(begin) > 1 or doc_text.count(end) > 1:
        raise SystemExit(
            "{} contains a duplicated marker ({} / {}); keep exactly "
            "one pair per file".format(target, begin, end)
        )
    pre = doc_text.split(begin)[0]
    post = doc_text.split(end)[1]
    return "{}{}\n{}\n{}{}".format(pre, begin, content, end, post)


def _called_name(call):
    """The called function's bare name for a Call node (Name or method Attribute)."""
    f = call.func
    if isinstance(f, ast.Name):
        return f.id
    if isinstance(f, ast.Attribute):
        return f.attr
    return None


def _all_functions(src_roots):
    """Map every function name in the source -> list of (module_rel, node,
    one-line summary), in scan order. A name can be defined in several modules —
    callers disambiguate (see build_flow)."""
    funcs = {}
    files, _ = _module_files(src_roots)
    for path, root_parent in files:
        try:
            tree = ast.parse(path.read_text(encoding="utf-8"))
        except SyntaxError:
            continue
        rel = (
            path.relative_to(root_parent)
            .with_suffix("")
            .as_posix()
            .replace("/__init__", "")
        )
        for node in ast.walk(tree):
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                funcs.setdefault(node.name, []).append(
                    (rel, node, first_line(ast.get_docstring(node)))
                )
    return funcs


def build_flow(src_roots, entry):
    """Ordered list of the internal functions the entry orchestrator calls.

    `entry` is a bare function name (`run`) or module-qualified (`module:run`,
    matching the module path's tail, e.g. `export/io:run` or just `io:run`).
    A bare name defined in more than one module is an error — qualify it."""
    funcs = _all_functions(src_roots)
    mod, _, name = entry.rpartition(":")
    candidates = funcs.get(name, [])
    if mod:
        candidates = [c for c in candidates if c[0] == mod or c[0].endswith("/" + mod)]
    if not candidates:
        raise SystemExit("flow entry function not found: {}".format(entry))
    if len(candidates) > 1:
        raise SystemExit(
            "flow entry {!r} is ambiguous — defined in: {}. Qualify it as "
            "'module:{}'.".format(name, ", ".join(c[0] for c in candidates), name)
        )
    _rel, node, summary = candidates[0]
    internal = set(funcs)
    calls = []
    for n in ast.walk(node):
        if isinstance(n, ast.Call):
            cn = _called_name(n)
            if cn and cn in internal and cn != name:
                calls.append((n.lineno, n.col_offset, cn))
    calls.sort()
    note = (
        "_Generated by `scripts/gen_arch_map.py --flow {0}` — the ordered "
        "internal calls in `{0}`. Keep entry points thin: a readable flow here "
        "means the orchestrator delegates instead of computing. Loops/branches "
        "are not shown — see the overview above for control flow._".format(name)
    )
    lines = [note, ""]
    if summary:
        lines.append("**`{}`** — {}".format(name, summary))
        lines.append("")
    if not calls:
        lines.append(
            "_(no internal calls found — is `{}` the orchestrator, and "
            "is its module under --src?)_".format(name)
        )
    else:
        for i, (_l, _c, cn) in enumerate(calls, 1):
            s = funcs[cn][0][2]  # first definition's summary (display only)
            lines.append("{}. `{}`{}".format(i, cn, " — " + s if s else ""))
    return "\n".join(lines)


def main():
    _utf8_console()
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument(
        "--src",
        action="append",
        default=None,
        help="source root to scan (repeatable; default: src)",
    )
    ap.add_argument(
        "--doc",
        action="append",
        default=None,
        help="file(s) to update; repeatable (default: docs/architecture.md). "
        "Point at AGENTS.md / CLAUDE.md too to route the map there.",
    )
    ap.add_argument(
        "--flow",
        default=None,
        help="entry/orchestrator function whose call sequence fills "
        "the GENERATED FLOW markers (e.g. 'run' or 'mod:run')",
    )
    ap.add_argument(
        "--check",
        action="store_true",
        help="do not write; exit 1 if any target is stale",
    )
    ap.add_argument(
        "--strict-parse",
        action="store_true",
        help="exit 1 if any scanned module fails to parse",
    )
    args = ap.parse_args()

    src_roots = args.src or ["src"]
    docs = [Path(d) for d in (args.doc or ["docs/architecture.md"])]
    # An empty scan is legitimate pre-code, but on a repo whose code lives in
    # another language the map — and its --check freshness gate — would pass
    # *vacuously* forever while the docs still promise drift-proofing. Say so
    # loudly rather than let the guarantee silently lapse (see ADOPTING.md).
    if not _module_files(src_roots)[0]:
        print(
            "gen_arch_map: WARNING - no source scanned under {} — the map is "
            "empty and --check passes vacuously. If this repo's code is in "
            "another language, port the generator to it (the marker block is "
            "the contract) or remove the arch-map step; see ADOPTING.md.".format(
                ", ".join(str(s) for s in src_roots)
            ),
            file=sys.stderr,
        )
    generated = build_map(src_roots)
    diagram = build_dependency_diagram(src_roots)
    flow = build_flow(src_roots, args.flow) if args.flow else None

    stale = False
    for doc in docs:
        if not doc.exists():
            raise SystemExit("target file not found: {}".format(doc))
        current = doc.read_text(encoding="utf-8")
        updated = splice_region(current, BEGIN, END, generated, doc, required=True)
        # DIAGRAM and FLOW markers are optional per doc — presence opts in.
        updated = splice_region(
            updated, BEGIN_DIAGRAM, END_DIAGRAM, diagram, doc, required=False
        )
        if flow is not None:
            updated = splice_region(
                updated, BEGIN_FLOW, END_FLOW, flow, doc, required=False
            )
        if args.check:
            if updated != current:
                stale = True
                print(
                    "code map STALE in {}: run `python scripts/gen_arch_map.py`".format(
                        doc
                    ),
                    file=sys.stderr,
                )
        elif updated != current:
            doc.write_text(updated, encoding="utf-8")
            print("code map regenerated -> {}".format(doc))
        else:
            print("code map already up to date -> {}".format(doc))

    strict_fail = False
    if args.strict_parse:
        for rel, msg in collect_parse_errors(src_roots):
            print(
                "strict-parse: {} failed to parse — {}".format(rel, msg),
                file=sys.stderr,
            )
            strict_fail = True

    if args.check and stale:
        sys.exit(1)
    if strict_fail:
        sys.exit(1)
    if args.check:
        print("code map up to date.")


if __name__ == "__main__":
    main()
