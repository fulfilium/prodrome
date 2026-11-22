#!/usr/bin/env python3
"""Find the uses of a module that a crate split must cut.

A crate can hold a set of modules only if no module of the set names a
module outside the set. This script reads the Rust files under a source
directory and finds each path that names a top-level module: a path
`crate::<module>`, each name in a group `crate::{a, b::c}`, and a path
`super::<name>` that reaches the crate root. It prints each use by a
module of the set of a module that is not in the set. It also prints each
use of the facade crate `prodrome` or of `prodrome_core`, which a lower
crate cannot name. It exits with status 1 if it prints a use.

Run it from the repository root:

    python3 scripts/edges.py data/src
    python3 scripts/edges.py core/src memo digest

The first argument is the source directory of a crate. The other
arguments name the top-level modules of the set. With no module named,
the set is every module of the directory, so the script prints only the
uses of the facade.

The script does not read comments, the text of string and character
literals, an item marked `#[cfg(test)]`, or the file `lib.rs`, which
declares the modules. A test module may name any module, because a
crate's tests are not part of its face. A `super::` inside an inline
`mod name { ... }` counts from the file's module, so the script reads it
one level too high; the crates of this workspace have no such use.
`scripts/test_edges.py` checks each form on the files under
`scripts/edges/`.
"""

import re
import sys
from pathlib import Path

ROOTS = ("crate", "prodrome", "prodrome_core")
WORD = re.compile(r"[A-Za-z_]\w*")


def module_path(src, path):
    """The module path of the file `path`, such as `["fold", "climb"]`."""
    parts = list(path.relative_to(src).parts)
    parts[-1] = parts[-1][:-3]
    if parts[-1] == "mod":
        parts.pop()
    return parts


def strip(text):
    """`text` with comments and the contents of literals blanked out.

    Each removed character becomes a space, and each line end stays, so
    that the line numbers of the result are the line numbers of `text`.
    """
    out = []
    i = 0
    n = len(text)

    def blank(part):
        return "".join("\n" if c == "\n" else " " for c in part)

    while i < n:
        c = text[i]
        if text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end == -1 else end
            out.append(blank(text[i:end]))
            i = end
        elif text.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if text.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif text.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            out.append(blank(text[i:j]))
            i = j
        elif re.match(r"b?r#*\"", text[i:]) and not (i and (text[i - 1].isalnum() or text[i - 1] == "_")):
            head = re.match(r"b?r(#*)\"", text[i:])
            close = '"' + head.group(1)
            end = text.find(close, i + head.end())
            end = n if end == -1 else end + len(close)
            out.append(blank(text[i:end]))
            i = end
        elif c == '"' or (c == "b" and text.startswith('b"', i)):
            j = i + (2 if c == "b" else 1)
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            out.append(blank(text[i:j + 1]))
            i = j + 1
        elif c == "'" and re.match(r"'(\\.[^']*|[^\\'])'", text[i:]):
            literal = re.match(r"'(\\.[^']*|[^\\'])'", text[i:]).group(0)
            out.append(blank(literal))
            i += len(literal)
        else:
            out.append(c)
            i += 1
    return "".join(out)


def without_tests(code):
    """`code` with each item marked `#[cfg(test)]` blanked out."""
    while True:
        mark = re.search(r"#\[cfg\(test\)\]", code)
        if not mark:
            return code
        j = mark.end()
        while j < len(code) and code[j] not in "{;":
            j += 1
        if j < len(code) and code[j] == "{":
            depth = 0
            while j < len(code):
                depth += {"{": 1, "}": -1}.get(code[j], 0)
                j += 1
                if depth == 0:
                    break
        else:
            j += 1
        part = code[mark.start():j]
        code = code[: mark.start()] + "".join("\n" if c == "\n" else " " for c in part) + code[j:]


def group_names(code, at):
    """The first names of each path in the group `{...}` that opens at `at`."""
    names = []
    depth = 0
    expect = True
    j = at
    while j < len(code):
        c = code[j]
        if c == "{":
            depth += 1
            expect = depth == 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                break
        elif c == "," and depth == 1:
            expect = True
        elif expect and depth == 1:
            word = WORD.match(code, j)
            if word:
                names.append(word.group(0))
                expect = False
                j = word.end()
                continue
        j += 1
    return names


def uses(code, here):
    """Each `(root, name, offset)` that `code` names, in the module `here`.

    `root` is `crate`, `prodrome` or `prodrome_core`, and `name` is the
    top-level module or item of that crate.
    """
    found = []
    for path in re.finditer(r"(?<![\w$])((?:super\s*::\s*)+|crate\s*::|prodrome\s*::|prodrome_core\s*::)\s*", code):
        head = path.group(1)
        if head.startswith("super"):
            depth = head.count("super")
            if depth < len(here):
                continue
            root = "crate"
        else:
            root = head.split(":")[0].strip()
        rest = path.end()
        if code.startswith("{", rest):
            for name in group_names(code, rest):
                found.append((root, name, path.start()))
        else:
            word = WORD.match(code, rest)
            if word:
                found.append((root, word.group(0), path.start()))
    return found


def edges(src, chosen=()):
    """Each `(file, line, root, name)` by which a chosen module names a
    module outside the set, or another crate of the workspace."""
    src = Path(src)
    files = sorted(p for p in src.rglob("*.rs") if p.relative_to(src).parts != ("lib.rs",))
    modules = {module_path(src, p)[0] for p in files}
    chosen = set(chosen) or modules
    unknown = chosen - modules
    if unknown:
        raise SystemExit(f"no such module in {src}: {', '.join(sorted(unknown))}")
    found = []
    for path in files:
        here = module_path(src, path)
        if here[0] not in chosen:
            continue
        code = without_tests(strip(path.read_text(encoding="utf-8")))
        for root, name, offset in uses(code, here):
            if root != "crate" or (name in modules and name not in chosen):
                found.append((path, code.count("\n", 0, offset) + 1, root, name))
    return found, chosen


def main():
    found, chosen = edges(sys.argv[1], sys.argv[2:])
    for path, line, root, name in found:
        print(f"{path}:{line}: {root}::{name}")
    if found:
        print(f"{len(found)} uses of a module outside the set", file=sys.stderr)
        sys.exit(1)
    print(f"no module of {', '.join(sorted(chosen))} names a module outside the set")


if __name__ == "__main__":
    main()
