#!/usr/bin/env python3
"""Doc navigability & staleness check: keep the hand-written doc set honest.

The harness already gates the freshness of *generated* blocks (the code map —
`gen_arch_map.py --check`), but nothing checks that the **hand-written** docs
stay navigable: that every intra-repo link resolves, every doc is reachable,
and a doc hasn't been frozen beside churning code. This operationalizes the
"verify no broken intra-doc links" step several gates ask a human to do by hand,
extending the "the map must stay honest" guarantee from code to docs
(process.md §3 "Reviewability").

Stdlib only, like trace.py / check_flows.py:

    python scripts/check_docs.py [--root .] [--docs docs] [--entry PATH ...]
                                 [--ignore GLOB ...] [--strict-orphans] [--stale]

It scans root-level `*.md` plus everything under `docs/`, then reports three
finding classes:

  - **broken intra-repo links** (target file/dir or `#anchor` missing) — a hard
    finding; exit 1.
  - **orphan docs** (no path from an entry root: root-level `*.md`, an optional
    `docs/index.md` Map-of-Content, or a `--entry`) — a warning by default; exit
    1 only with `--strict-orphans`. A fresh scaffold legitimately has standalone
    docs (interfaces, stakeholder-needs) until the project links them, so the
    floor is warn, not fail.
  - **staleness** (`--stale`, git-gated, warn-only): a doc linking a *non-doc*
    file (source/asset) that was committed more recently than the doc itself —
    a "lying map" heuristic. Degrades to a clean skip when git is unavailable or
    the tree isn't a git work tree.

Scope (the high-value 80%): inline links `[text](dest)` and same-file/`file#frag`
anchors against GitHub-style heading slugs (plus `{#custom-id}` suffixes and
`<a name=...>`/`id=...`). Out of scope by design: reference-style links
(`[t][ref]`), images (`![alt](src)` — skipped, not existence-checked), and links
inside fenced/inline code (stripped before parsing). Anchors are only validated
against Markdown targets the script can parse.
"""

import argparse
import re
import shutil
import subprocess
import sys
from pathlib import Path
from urllib.parse import unquote


def _utf8_console():
    """Emit UTF-8 to stdout/stderr whatever the OS console codepage is, so a
    non-ASCII doc path or link target in a finding can't raise
    UnicodeEncodeError on a legacy Windows cp1252 console. Python 3.7+ streams
    expose `.reconfigure`; guard for the rest."""
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8")
        except (AttributeError, ValueError):
            pass


# A fenced code block opens/closes on a line of >=3 backticks or tildes.
FENCE_RE = re.compile(r"^\s*(`{3,}|~{3,})")
# Inline code span: backtick-delimited; stripped so `[x](y)` in code isn't a link.
INLINE_CODE_RE = re.compile(r"`[^`]*`")
# ATX heading: 1-6 leading #, optional trailing #s.
HEADING_RE = re.compile(r"^(#{1,6})\s+(.*?)\s*#*\s*$")
# Inline link: optional leading ! (image), [text](dest "optional title").
# dest is either <bracketed> or a run with no whitespace/closing paren.
LINK_RE = re.compile(r"(!?)\[[^\]]*\]\(\s*(<[^>]*>|[^)\s]*)(?:\s+[^)]*)?\)")
# Explicit anchors: a `{#custom-id}` heading suffix and HTML name=/id= attrs.
CUSTOM_ID_RE = re.compile(r"\{#([\w-]+)\}\s*$")
HTML_ANCHOR_RE = re.compile(r"""<a\s[^>]*\b(?:name|id)\s*=\s*["']([^"']+)["']""", re.I)
# A URL scheme (http:, mailto:, …) or protocol-relative // marks an external link.
EXTERNAL_RE = re.compile(r"^(?:[A-Za-z][A-Za-z0-9+.\-]*:|//)")
MD_SUFFIXES = (".md", ".markdown")


def slugify(text):
    """GitHub-style heading slug: lowercase, drop punctuation, spaces->hyphens.

    Each whitespace run-member becomes its own hyphen (so "A & B" -> "a--b"),
    matching github-slugger closely enough for intra-doc anchor links.
    """
    s = text.strip()
    s = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", s)  # [text](url) -> text
    s = s.replace("`", "").replace("*", "").replace("_", "")
    s = s.lower()
    s = re.sub(r"[^\w\s-]", "", s)  # keep word chars, whitespace, hyphen
    s = re.sub(r"\s", "-", s)
    return s


def blank_fenced(text):
    """Return the lines of `text` with fenced code blocks blanked out (line
    count preserved so reported line numbers stay accurate)."""
    out = []
    fence = None
    for line in text.splitlines():
        m = FENCE_RE.match(line)
        if fence is None:
            if m:
                fence = m.group(1)[0]
                out.append("")
            else:
                out.append(line)
        else:
            if m and m.group(1)[0] == fence:
                fence = None
            out.append("")
    return out


def parse_doc(path):
    """Parse one Markdown file into its outbound links and its anchor set."""
    text = path.read_text(encoding="utf-8")
    links = []  # (lineno, dest)
    anchors = set()  # slugs/ids this doc exposes as #fragments
    seen = {}  # slug -> count, for GitHub's -1/-2 disambiguation
    for i, line in enumerate(blank_fenced(text), 1):
        for m in HTML_ANCHOR_RE.finditer(line):
            anchors.add(m.group(1).lower())
        heading = HEADING_RE.match(line)
        if heading:
            title = heading.group(2)
            cid = CUSTOM_ID_RE.search(title)
            if cid:
                anchors.add(cid.group(1).lower())
                title = CUSTOM_ID_RE.sub("", title)
            slug = slugify(title)
            if slug:
                n = seen.get(slug, 0)
                anchors.add(slug if n == 0 else "{}-{}".format(slug, n))
                seen[slug] = n + 1
            continue
        cleaned = INLINE_CODE_RE.sub("", line)
        for m in LINK_RE.finditer(cleaned):
            if m.group(1) == "!":
                continue  # image: out of scope (see module docstring)
            dest = m.group(2).strip()
            if dest.startswith("<") and dest.endswith(">"):
                dest = dest[1:-1].strip()
            if dest:
                links.append((i, dest))
    return {"links": links, "anchors": anchors}


def collect_docs(root, docs_dir, ignore=()):
    """Root-level *.md plus every *.md under docs/, de-duplicated and resolved.

    Paths matching an `ignore` glob (repo-relative POSIX) are dropped entirely —
    not parsed, not orphan-reported — so generated composites (the gitignored
    `docs/test/report.md`) don't show up as findings. They still resolve as link
    *targets*, since the file is on disk.
    """
    found = {}
    for p in sorted(root.glob("*.md")):
        found[p.resolve()] = True
    dd = root / docs_dir
    if dd.is_dir():
        for p in sorted(dd.rglob("*.md")):
            found[p.resolve()] = True
    docs = []
    for p in found:
        relpath = rel(p, root)
        if any(Path(relpath).match(g) for g in ignore):
            continue
        docs.append(p)
    return docs


def rel(path, root):
    """Display path: repo-relative POSIX, or absolute if outside the root."""
    try:
        return path.relative_to(root).as_posix()
    except ValueError:
        return path.as_posix()


def check_links(parsed, docs, root):
    """Validate every outbound link; build the doc->doc reachability graph.

    Returns (broken, graph): broken is a list of (src, lineno, dest, reason);
    graph maps each scanned doc to the set of scanned docs it links to.
    """
    docset = set(docs)
    broken = []
    graph = {d: set() for d in docs}
    anchor_cache = dict(parsed)  # memoize on-demand parses of off-set md targets
    for d, info in parsed.items():
        src = rel(d, root)
        for lineno, dest in info["links"]:
            if dest.startswith("#"):  # same-file anchor
                anchor = dest[1:].lower()
                if anchor and anchor not in info["anchors"]:
                    broken.append((src, lineno, dest, "no such anchor in this doc"))
                continue
            if EXTERNAL_RE.match(dest):
                continue
            path_part, _, frag = dest.partition("#")
            if not path_part:
                continue
            target = (d.parent / unquote(path_part)).resolve()
            if not target.exists():
                broken.append((src, lineno, dest, "target not found"))
                continue
            if target.is_dir():
                continue  # a link to a directory that exists is fine
            if target in docset:
                graph[d].add(target)
            if frag:
                tinfo = anchor_cache.get(target)
                if tinfo is None and target.suffix.lower() in MD_SUFFIXES:
                    tinfo = parse_doc(target)
                    anchor_cache[target] = tinfo
                if tinfo is not None and frag.lower() not in tinfo["anchors"]:
                    broken.append((src, lineno, dest, "no such anchor in target"))
    return broken, graph


def entry_roots(root, docs, docs_dir, extra):
    """Reachability roots: top-level *.md, an optional docs/index.md Map-of-
    Content, and any explicit --entry. These are never reported as orphans."""
    roots = set()
    docset = set(docs)
    for d in docs:
        if d.parent == root:
            roots.add(d)
    moc = (root / docs_dir / "index.md").resolve()
    if moc in docset:
        roots.add(moc)
    for e in extra:
        roots.add((root / e).resolve())
    return roots


def reachable(roots, graph):
    """Docs reachable from any entry root by following doc->doc links."""
    seen = set()
    stack = [r for r in roots if r in graph]
    while stack:
        node = stack.pop()
        if node in seen:
            continue
        seen.add(node)
        stack.extend(graph.get(node, ()))
    return seen


def find_orphans(docs, graph, roots, root):
    """Scanned docs with no path from an entry root (entry roots excepted)."""
    reached = reachable(roots, graph)
    orphans = [rel(d, root) for d in docs if d not in roots and d not in reached]
    return sorted(orphans)


def git_commit_lookup(root):
    """Return a memoized path->last-commit-epoch lookup, or None when git is
    unavailable or `root` isn't inside a git work tree (so --stale degrades to a
    clean skip rather than failing)."""
    if not shutil.which("git"):
        return None
    probe = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "--is-inside-work-tree"],
        capture_output=True,
        text=True,
    )
    if probe.returncode != 0 or probe.stdout.strip() != "true":
        return None
    cache = {}

    def lookup(path):
        if path in cache:
            return cache[path]
        out = subprocess.run(
            ["git", "-C", str(root), "log", "-1", "--format=%ct", "--", str(path)],
            capture_output=True,
            text=True,
        )
        val = int(out.stdout.strip()) if out.stdout.strip().isdigit() else None
        cache[path] = val
        return val

    return lookup


def find_stale(parsed, root, lookup):
    """Docs linking a non-doc file committed more recently than the doc itself.

    `lookup` is path->epoch (injectable for tests). Doc-to-doc links are skipped
    (too noisy a signal); unresolvable/untracked targets degrade to skip.
    """
    stale = []
    for d, info in parsed.items():
        d_time = lookup(d)
        if d_time is None:
            continue
        for lineno, dest in info["links"]:
            if dest.startswith("#") or EXTERNAL_RE.match(dest):
                continue
            path_part = dest.partition("#")[0]
            if not path_part:
                continue
            target = (d.parent / unquote(path_part)).resolve()
            if not target.is_file() or target.suffix.lower() in MD_SUFFIXES:
                continue
            t_time = lookup(target)
            if t_time is not None and t_time > d_time:
                stale.append((rel(d, root), lineno, dest))
    return stale


def main():
    _utf8_console()
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--root", default=".", help="repo root (default: .)")
    ap.add_argument(
        "--docs", default="docs", help="docs subdirectory to scan (default: docs)"
    )
    ap.add_argument(
        "--entry",
        action="append",
        default=[],
        metavar="PATH",
        help="extra entry-root doc for reachability (repeatable)",
    )
    ap.add_argument(
        "--ignore",
        action="append",
        default=[],
        metavar="GLOB",
        help="repo-relative glob to drop from the scanned set, e.g. a generated "
        "composite like docs/test/report.md (repeatable)",
    )
    ap.add_argument(
        "--strict-orphans",
        action="store_true",
        help="treat orphan docs as failures, not warnings",
    )
    ap.add_argument(
        "--stale",
        action="store_true",
        help="also warn on docs older than the non-doc files they link (needs git)",
    )
    args = ap.parse_args()

    root = Path(args.root).resolve()
    docs = collect_docs(root, args.docs, args.ignore)
    if not docs:
        print("check_docs: OK - no Markdown docs found under {}".format(root))
        return
    parsed = {d: parse_doc(d) for d in docs}

    broken, graph = check_links(parsed, docs, root)
    roots = entry_roots(root, docs, args.docs, args.entry)
    orphans = find_orphans(docs, graph, roots, root)

    for src, lineno, dest, reason in broken:
        print(
            "check_docs: FAIL - {}:{} broken link -> {} ({})".format(
                src, lineno, dest, reason
            )
        )
    for o in orphans:
        print(
            "check_docs: {} - orphan doc (no path from an entry root): {}".format(
                "FAIL" if args.strict_orphans else "WARN", o
            )
        )

    if args.stale:
        lookup = git_commit_lookup(root)
        if lookup is None:
            print(
                "check_docs: staleness check skipped "
                "(git unavailable or not a git work tree)"
            )
        else:
            for src, lineno, dest in find_stale(parsed, root, lookup):
                print(
                    "check_docs: WARN - possibly stale: {}:{} links {} "
                    "(changed after the doc)".format(src, lineno, dest)
                )

    failed = broken or (args.strict_orphans and orphans)
    n_links = sum(
        1
        for info in parsed.values()
        for _, dest in info["links"]
        if not EXTERNAL_RE.match(dest)
    )
    if failed:
        print(
            "check_docs: FAIL - {} broken link(s), {} orphan(s) across {} doc(s).".format(
                len(broken), len(orphans), len(docs)
            )
        )
        sys.exit(1)
    print(
        "check_docs: OK - {} doc(s), {} intra-repo link(s), 0 broken{}.".format(
            len(docs),
            n_links,
            " ({} orphan warning(s))".format(len(orphans)) if orphans else "",
        )
    )


if __name__ == "__main__":
    main()
