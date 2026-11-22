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


def main():
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
