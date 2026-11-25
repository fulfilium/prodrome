#!/usr/bin/env python3
"""Check that each file path that a document names exists.

The script reads `AGENTS.md`, `CONTRIBUTING.md` and `README.md`, or the
files that the command names, and finds each span in backticks that looks
like a file of this repository: a span of path characters that ends in
one of the extensions below. It prints each such path that does not exist,
relative to the repository root, and exits with status 1 if it prints one.

Run it from the repository root:

    python3 scripts/doc_paths.py [file ...]

A span with a space, a glob (`*`), a placeholder (`<`) or a scheme
(`://`) is not a path.
"""

import re
import sys
from pathlib import Path

DOCS = ("AGENTS.md", "CONTRIBUTING.md", "README.md")
PATH = re.compile(r"[A-Za-z0-9_.\-/]+\.(rs|md|py|toml|nix|yml|yaml|json|sh|lock|typ|html|js)")


def missing(doc):
    """Each path in backticks in `doc` that names no file."""
    text = Path(doc).read_text(encoding="utf-8")
    for span in re.findall(r"`([^`\n]+)`", text):
        if PATH.fullmatch(span) and not Path(span).exists():
            yield span


def main():
    found = [(doc, path) for doc in sys.argv[1:] or DOCS for path in missing(doc)]
    for doc, path in found:
        print(f"{doc}: `{path}` names no file")
    sys.exit(1 if found else 0)


if __name__ == "__main__":
    main()
