#!/usr/bin/env python3
"""Measure the public face of `prodrome-core`.

The public face of a crate is what a reader of its public API reads: the
signature of each public item and its doc comment. This script reads the
JSON that rustdoc writes for the crate, walks every item that is reachable
from the crate root, and prints the characters of signatures and of docs,
by top-level module.

Run it from the repository root:

    RUSTC_BOOTSTRAP=1 nix develop -c cargo rustdoc -p prodrome-core --lib \
        --target-dir target/face -- -Z unstable-options --output-format json
    python3 scripts/face.py target/face/doc/prodrome.json

Add `--by-file` to count by source file instead of by top-level module.
Add `--all-features` to the `cargo rustdoc` line to count the `laws`
module, which is behind a feature.

`RUSTC_BOOTSTRAP=1` lets the stable toolchain write rustdoc's JSON, which
is an unstable output format. The script counts:

- for a function, the source text of its signature, up to its body;
- for a struct, an enum, a trait or a union, the text of its header;
- for a field, a variant, a type alias, a constant or a re-export, its text;
- for a module, its doc comment only;
- the methods and associated items of inherent impls and of traits.

It does not count trait impls (`impl Display for Hash`), because their
methods carry no doc comment of their own. It collapses each run of white
space in a signature to one space. A token is four characters.

The second mode, `--lines`, needs no rustdoc. It reads the source files
under `core/src` line by line, in the manner of a tool that an application
uses to give each dependency a reading budget:

    python3 scripts/face.py --lines [repository root]

It counts each line as written, with its prefix and its indentation, and
one character for its line end:

- every module doc line (`//!`);
- every item that starts with `pub` (also `pub(crate)` and `pub(super)`)
  or `impl`, whatever the visibility of its module, with the doc lines and
  the attributes above it;
- for a function or a one-line item, the text up to its first `{` or `;`;
- for a struct, an enum, a trait, a union or an impl block, its whole
  body, with each method's signature and doc but without the method's
  body.
"""

import json
import re
import sys
from collections import defaultdict
from pathlib import Path

TOKEN = 4


def kind_of(item):
    inner = item["inner"]
    return next(iter(inner)) if isinstance(inner, dict) else inner


def source_text(item, cache):
    span = item.get("span")
    if not span:
        return ""
    path = Path(span["filename"])
    if path not in cache:
        cache[path] = path.read_text(encoding="utf-8").split("\n")
    lines = cache[path]
    (l0, c0), (l1, c1) = span["begin"], span["end"]
    if l0 == l1:
        return lines[l0 - 1][c0:c1]
    part = [lines[l0 - 1][c0:]] + lines[l0:l1 - 1] + [lines[l1 - 1][:c1]]
    return "\n".join(part)


def strip_attrs(text):
    kept = [
        line for line in text.split("\n")
        if not line.strip().startswith(("#[", "///", "//!", "//"))
    ]
    return "\n".join(kept)


def head(text, stops):
    cut = len(text)
    for stop in stops:
        at = text.find(stop)
        if at != -1:
            cut = min(cut, at)
    return text[:cut]


def signature(item, kind, cache):
    text = strip_attrs(source_text(item, cache))
    if kind == "module":
        return ""
    if kind in ("function", "struct", "enum", "trait", "union", "macro"):
        text = head(text, ["{", ";"] if kind != "macro" else ["{", "("])
    elif kind in ("variant",):
        text = head(text, ["{"])
    return re.sub(r"\s+", " ", text).strip().rstrip(",")


def module_of(item, by_file):
    span = item.get("span")
    if not span:
        return "?"
    parts = Path(span["filename"]).parts
    if "src" not in parts:
        return "?"
    rest = parts[parts.index("src") + 1:]
    if by_file:
        return "/".join(rest)
    name = rest[0]
    return name[:-3] if name.endswith(".rs") else name


def walk(index, root):
    seen = set()
    stack = [root]
    while stack:
        key = stack.pop()
        if key in seen or key not in index:
            continue
        item = index[key]
        if item["crate_id"] != 0:
            continue
        seen.add(key)
        kind = kind_of(item)
        inner = item["inner"][kind] if isinstance(item["inner"], dict) else {}
        children = []
        if kind == "module":
            children = inner["items"]
        elif kind == "struct":
            shape = inner["kind"]
            if isinstance(shape, dict):
                if "plain" in shape:
                    children = list(shape["plain"]["fields"])
                elif "tuple" in shape:
                    children = [f for f in shape["tuple"] if f is not None]
            children += inner["impls"]
        elif kind == "enum":
            children = inner["variants"] + inner["impls"]
        elif kind == "union":
            children = inner["fields"] + inner["impls"]
        elif kind == "variant":
            shape = inner["kind"]
            if isinstance(shape, dict):
                if "struct" in shape:
                    children = shape["struct"]["fields"]
                elif "tuple" in shape:
                    children = [f for f in shape["tuple"] if f is not None]
        elif kind == "trait":
            children = inner["items"]
        elif kind == "impl":
            if inner["trait"] is not None or inner["is_synthetic"] \
                    or inner["blanket_impl"] is not None:
                seen.discard(key)
                continue
            children = inner["items"]
        elif kind == "use":
            if inner.get("id") is not None:
                children = [inner["id"]]
        stack.extend(str(c) for c in children)
    return seen


def by_lines(root):
    """The `--lines` measure: characters by top-level module."""
    total = defaultdict(int)
    src = Path(root, "core", "src")
    for path in sorted(src.rglob("*.rs")):
        lines = path.read_text(encoding="utf-8").split("\n")
        name = path.relative_to(src).parts[0]
        module = name[:-3] if name.endswith(".rs") else name
        above = 0
        i = 0
        while i < len(lines):
            line = lines[i]
            text = line.strip()
            if text.startswith("//!"):
                total[module] += len(line) + 1
            elif text.startswith(("///", "#[")):
                above += len(line) + 1
            elif re.match(r"(pub[\s(]|impl\b)", text):
                found = re.search(r"\b(struct|enum|trait|union)\b", text)
                if text.startswith("impl") or found:
                    size, i = body(lines, i)
                else:
                    size, i = head_of(lines, i)
                total[module] += above + size
                above = 0
                continue
            else:
                above = 0
            i += 1
    return total


def head_of(lines, i):
    """The text of lines from `i` up to the first `{` or `;`."""
    size = 0
    while i < len(lines):
        line = lines[i]
        stops = [at for at in (line.find("{"), line.find(";")) if at != -1]
        if stops:
            return size + min(stops) + 2, i + 1
        size += len(line) + 1
        i += 1
    return size, i


def body(lines, i):
    """A type's, a trait's or an impl's lines, without method bodies."""
    size = 0
    depth = 0
    while i < len(lines):
        line = lines[i]
        text = line.strip()
        if depth == 1 and re.search(r"\bfn\b", text) and "{" in text:
            size += line.index("{") + 1
            open_ = line.count("{") - line.count("}")
            i += 1
            while open_ > 0 and i < len(lines):
                open_ += lines[i].count("{") - lines[i].count("}")
                i += 1
            continue
        size += len(line) + 1
        depth += line.count("{") - line.count("}")
        i += 1
        if depth <= 0 and ("}" in line or line.rstrip().endswith(";")):
            break
    return size, i


def main_lines(root):
    total = by_lines(root)
    print(f"{'module':<14}{'chars':>10}{'tokens':>9}")
    for m in sorted(total, key=lambda m: -total[m]):
        print(f"{m:<14}{total[m]:>10}{total[m] // TOKEN:>9}")
    print(f"{'total':<14}{sum(total.values()):>10}"
          f"{sum(total.values()) // TOKEN:>9}")


def main():
    if sys.argv[1:2] == ["--lines"]:
        main_lines(sys.argv[2] if len(sys.argv) > 2 else ".")
        return
    data = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    by_file = sys.argv[2:] == ["--by-file"]
    index = data["index"]
    cache = {}
    sig = defaultdict(int)
    doc = defaultdict(int)
    count = defaultdict(int)
    items = []
    for key in walk(index, str(data["root"])):
        item = index[key]
        kind = kind_of(item)
        if kind == "impl":
            continue
        s = len(signature(item, kind, cache))
        d = len(item.get("docs") or "")
        m = module_of(item, by_file)
        sig[m] += s
        doc[m] += d
        count[m] += 1
        name = item.get("name") or "?"
        items.append((s + d, m, name, kind))
    width = max(len(m) for m in sig) + 2
    print(f"{'module':<{width}}{'items':>7}{'sig chars':>11}{'doc chars':>11}"
          f"{'tokens':>9}")
    for m in sorted(sig, key=lambda m: -(sig[m] + doc[m])):
        tokens = (sig[m] + doc[m]) // TOKEN
        print(f"{m:<{width}}{count[m]:>7}{sig[m]:>11}{doc[m]:>11}{tokens:>9}")
    ts, td = sum(sig.values()), sum(doc.values())
    print(f"{'total':<{width}}{sum(count.values()):>7}{ts:>11}{td:>11}"
          f"{(ts + td) // TOKEN:>9}")
    print("\nlargest items (chars, module, name, kind):")
    for size, m, name, kind in sorted(items, reverse=True)[:10]:
        print(f"{size:>7}  {m}::{name} ({kind})")


if __name__ == "__main__":
    main()
