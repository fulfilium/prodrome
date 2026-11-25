#!/usr/bin/env python3
"""Check that each file path and each directory path that a document names
exists.

The script reads `AGENTS.md`, `CONTRIBUTING.md`, `README.md` and each
Markdown file under `docs/`, or the files that the command names. It finds
each span in backticks that looks like a path of this repository:

- a file: a span of path characters that ends in one of the extensions
  below;
- a directory: a span of path characters that ends in `/`.

It prints each such path that does not exist, relative to the repository
root, and exits with status 1 if it prints one.

Run it from the repository root, in a git checkout:

    python3 scripts/doc_paths.py [file ...]

A span with a space, a glob (`*`), a placeholder (`<`) or a scheme
(`://`) is not a path.
"""

import posixpath
import re
import subprocess
import sys
from pathlib import Path

DOCS = ("AGENTS.md", "CONTRIBUTING.md", "README.md", *sorted(map(str, Path("docs").glob("*.md"))))
FILE = re.compile(r"[A-Za-z0-9_.\-/]+\.(rs|md|py|toml|nix|yml|yaml|json|sh|lock|typ|html|js)")
DIRECTORY = re.compile(r"[A-Za-z0-9_.\-]+(/[A-Za-z0-9_.\-]+)*/")


# What a document may name that git does not track. A store writes these, a
# build writes these, or they are on another branch.
UNTRACKED = {
    "objects/",
    "roadmap/objects/",
    "refs/",
    "quarantine/",
    "roadmap/.lock",
    "root/.lock",
    "index.json",
    "target/",
    "result/web/",
    "result/nodejs/",
    "nodejs/",
    "app.js",
    "sw.js",
    "PRICING.md",
}

# A path that a design note names as it was when the note was written. The
# value is where the file is now, or `None` for a file that a note proposed
# and no commit made.
HISTORICAL = {
    "core/src/registers.rs": "dag/src/registers.rs",
    "core/src/fold.rs": "reading/src/fold.rs",
    "core/src/view.rs": "reading/src/view.rs",
    "core/src/store.rs": "store/src/store.rs",
    "core/src/store/": "store/src/store/",
    "core/src/snapshot.rs": "store/src/snapshot.rs",
    "core/src/event.rs": "data/src/event.rs",
    "core/src/fpl.rs": "fpl/src/fpl.rs",
    "core/src/data/": "data/src/",
    "core/src/instant.rs": "data/src/instant.rs",
    "core/src/memo/": "memo/src/",
    "store/husk.rs": "data/src/husk.rs",
    "fold/write.rs": "todo/src/todo/write.rs",
    "store/directory.rs": None,
}


def tracked():
    """The files that git tracks, and the directories that hold them, as
    paths from the repository root."""
    files = subprocess.run(
        ["git", "ls-files"], check=True, capture_output=True, text=True
    ).stdout.splitlines()
    directories = {posixpath.dirname(file) for file in files}
    directories |= {posixpath.dirname(directory) for directory in directories}
    return set(files), {directory for directory in directories if directory}


def names(span, doc, known):
    """Whether `span` names one of `known`: from the root, from the directory
    of `doc`, inside the `src` of a crate (`tags/alias.rs`), or as the end of
    a path, as a note names a file by its name alone. A path of `UNTRACKED`
    or `HISTORICAL` is also a name."""
    path = span.strip("/")
    crate, _, rest = path.partition("/")
    options = {
        path,
        posixpath.normpath(posixpath.join(posixpath.dirname(doc), path)),
        f"{crate}/src/{rest}".rstrip("/"),
    }
    return (
        span in UNTRACKED
        or span in HISTORICAL
        or any(
            option in known or entry.endswith("/" + path)
            for option in options
            for entry in known
        )
    )


def stale(files, directories):
    """Each entry of `HISTORICAL` that is wrong: a path that exists again, or
    a destination that does not exist."""
    known = files | directories
    for old, new in HISTORICAL.items():
        if old.rstrip("/") in known:
            yield f"HISTORICAL: `{old}` exists"
        if new is not None and new.rstrip("/") not in known:
            yield f"HISTORICAL: `{new}`, where `{old}` moved, does not exist"


def missing(doc, files, directories):
    """Each file path or directory path in backticks in `doc` that names
    nothing in the repository."""
    text = Path(doc).read_text(encoding="utf-8")
    for span in re.findall(r"`([^`\n]+)`", text):
        if FILE.fullmatch(span) and not names(span, doc, files):
            yield span
        elif DIRECTORY.fullmatch(span) and not names(span, doc, directories):
            yield span


def main():
    files, directories = tracked()
    found = [
        (doc, path)
        for doc in sys.argv[1:] or DOCS
        for path in missing(doc, files, directories)
    ]
    problems = list(stale(files, directories))
    for doc, path in found:
        print(f"{doc}: `{path}` names nothing")
    for problem in problems:
        print(problem)
    sys.exit(1 if found or problems else 0)


if __name__ == "__main__":
    main()
