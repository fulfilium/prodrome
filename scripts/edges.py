#!/usr/bin/env python3
"""Find the uses of a module that a crate split must cut.

A crate can hold a set of modules only if no module of the set names a
module outside the set. This script reads the Rust files under a source
directory, finds each path `crate::<module>` in the code, and prints each
use by a module of the set of a module that is not in the set. It also
prints each use of the facade crate `prodrome` or of `prodrome_core`,
which a lower crate cannot name. It exits with status 1 if it prints a
use.

Run it from the repository root:

    python3 scripts/edges.py data/src
    python3 scripts/edges.py core/src memo digest

The first argument is the source directory of a crate. The other
arguments name the top-level modules of the set. With no module named,
the set is every module of the directory, so the script prints only the
uses of the facade.

The script does not read comments, the `#[cfg(test)]` module at the end
of a file, the file `lib.rs`, which declares the modules, or a file that
the code includes as text with `include_str!`. A test module may name any
module, because a crate's tests are not part of its face. A file included
as text is an example in a doc comment, which names the crate as a
program that uses it does.
"""

import re
import sys
from pathlib import Path

PATH = re.compile(r"(?<![\w$])(crate|prodrome|prodrome_core)::(\w+)")


def module_of(src, path):
    """The top-level module that `path` belongs to."""
    first = path.relative_to(src).parts[0]
    return first[:-3] if first.endswith(".rs") else first


def code_lines(path):
    """The lines of `path` without comments and without its test module."""
    text = path.read_text(encoding="utf-8")
    cut = re.search(r"^#\[cfg\(test\)\]\s*\nmod tests\b", text, re.M)
    if cut:
        text = text[: cut.start()]
    for number, line in enumerate(text.split("\n"), 1):
        code = re.sub(r"//.*", "", line).strip()
        if code:
            yield number, code


def main():
    src = Path(sys.argv[1])
    texts = set()
    for path in src.rglob("*.rs"):
        for name in re.findall(r'include_str!\("([^"]+)"\)', path.read_text(encoding="utf-8")):
            texts.add((path.parent / name).resolve())
    files = sorted(
        p
        for p in src.rglob("*.rs")
        if p.relative_to(src).parts != ("lib.rs",) and p.resolve() not in texts
    )
    modules = {module_of(src, p) for p in files}
    chosen = set(sys.argv[2:]) or modules
    unknown = chosen - modules
    if unknown:
        sys.exit(f"no such module in {src}: {', '.join(sorted(unknown))}")
    found = 0
    for path in files:
        if module_of(src, path) not in chosen:
            continue
        for number, code in code_lines(path):
            for root, name in PATH.findall(code):
                outside = root != "crate" or (name in modules and name not in chosen)
                if outside:
                    found += 1
                    print(f"{path}:{number}: {root}::{name}: {code}")
    if found:
        print(f"{found} uses of a module outside the set", file=sys.stderr)
        sys.exit(1)
    print(f"no module of {', '.join(sorted(chosen))} names a module outside the set")


if __name__ == "__main__":
    main()
