#!/usr/bin/env python3
"""List which code names each public item of a crate of the library.

The script answers one question for every public item that rustdoc lists for
a crate: which code outside the item's own crate names it. It works on a
copy of the repository, so it changes no file of the tree.

Run it from the repository root:

    python3 scripts/outside.py              # one line for each item
    python3 scripts/outside.py --summary    # a count for each crate

The steps are:

1. For each crate of the library, `cargo rustdoc` writes the JSON of the
   crate, with the items that are `#[doc(hidden)]`. `face.py` walks it to
   list the public items.
2. The script copies the tracked files to a new directory under the system
   temporary directory, and puts `#[deprecated(note = "@@N")]` before each
   item. N is the number of the item.
3. `cargo check` compiles every target of the workspace in the copy, with
   every feature. The compiler warns at each place that names a deprecated
   item, and the note of the warning gives N. The place is the outermost
   call site of a macro, so a use in code that a macro writes counts at the
   macro call.
4. The script sorts each place into a class, by the file's path.

A use inside the item's own crate (`<crate>/src`) does not count. The
compiler does not warn at a use in code that `#[derive(Literal)]` writes,
so the script adds the paths that the derive's source names under
`literal::codec` to the users of the items of the data crate.

The classes of users, in the order of the output:

    cli, github, wasm, wasm exports, examples
                 programs of this repository, which a reader may copy
    law kit      `core/src/laws`, which a host runs in its own tests
    <crate>      another crate of the library, by its name
    tests        `tests/` and `benches/` of any crate

The script does not read documents. A document that names an item (the
specification, the guide, a doc test) needs a search of its own.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import face  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
CRATES = ["data", "memo", "dag", "declared", "store", "tags", "nest",
          "digest", "term", "fpl", "todo", "reading", "core"]
LIBRARY = set(CRATES) - {"core"}
KINDS = {"function", "struct", "enum", "trait", "type_alias", "constant",
         "assoc_const", "struct_field", "variant"}
PROGRAMS = ["cli", "github", "wasm exports", "wasm", "examples"]


def run(command, cwd, env=None, check=True):
    done = subprocess.run(command, cwd=cwd, capture_output=True, text=True,
                          env={**os.environ, **(env or {})})
    if check and done.returncode != 0:
        sys.exit(f"{' '.join(command)} failed:\n{done.stderr[-2000:]}")
    return done


def items_of(crate, target):
    """The public items that `crate` defines, with their places."""
    package = "prodrome-" + crate
    run(["cargo", "rustdoc", "-p", package, "--lib", "--all-features",
         "--target-dir", target, "--", "-Z", "unstable-options",
         "--output-format", "json", "--document-hidden-items"],
        ROOT, env={"RUSTC_BOOTSTRAP": "1"})
    name = "prodrome" if crate == "core" else "prodrome_" + crate
    data = json.loads(Path(target, "doc", name + ".json").read_text())
    index, cache, found = data["index"], {}, []
    for key in face.walk(index, str(data["root"])):
        item = index[key]
        kind = face.kind_of(item)
        span = item.get("span")
        if kind not in KINDS or not span:
            continue
        if not span["filename"].startswith(crate + "/src/"):
            continue
        found.append({
            "crate": crate, "file": span["filename"],
            "line": span["begin"][0], "column": span["begin"][1] - 1,
            "kind": kind, "name": item.get("name"),
            "hidden": any("doc(hidden)" in str(a) for a in item["attrs"]),
        })
    return found


def label(path, crate):
    """The class of the file `path`, as a user of an item of `crate`."""
    parts = path.split("/")
    if path.startswith(crate + "/src/"):
        return None
    if path.startswith("core/src/laws"):
        return "law kit"
    if "tests" in parts or "benches" in parts:
        return "tests"
    if path.startswith("core/examples"):
        return "examples"
    if path.startswith("wasm/exports"):
        return "wasm exports"
    if parts[0] in ("cli", "github", "wasm"):
        return parts[0]
    if parts[0] in LIBRARY or path.startswith("core/src"):
        return "prodrome-" + parts[0]
    return path


def outermost(span):
    while span.get("expansion"):
        span = span["expansion"]["span"]
    return span["file_name"]


def mark(items, copy):
    """Put the attribute before each item in the copy."""
    by_file = defaultdict(list)
    for number, item in enumerate(items):
        item["number"] = number
        by_file[item["file"]].append(item)
    for name, group in by_file.items():
        lines = (ROOT / name).read_text().split("\n")
        for item in sorted(group, key=lambda i: (i["line"], -i["column"])):
            text = lines[item["line"] - 1]
            column = item["column"]
            starts = text[column:]
            named = (starts.startswith("pub ")
                     or item["kind"] == "variant" and starts[:1].isalpha()
                     or re.match(r"(fn|const|type) ", starts))
            if not named:
                item["skipped"] = True
                continue
            attribute = f'#[deprecated(note = "@@{item["number"]}")] '
            lines[item["line"] - 1] = text[:column] + attribute + text[column:]
        Path(copy, name).write_text("\n".join(lines))


def derive_paths():
    """The names under `codec` that the derive's output calls."""
    names = set()
    for path in (ROOT / "derive" / "src").glob("*.rs"):
        names |= set(re.findall(r"codec::(\w+)", path.read_text()))
    return names


def main():
    summary = sys.argv[1:] == ["--summary"]
    work = tempfile.mkdtemp(prefix="prodrome-outside-")
    try:
        report(work, summary)
    finally:
        shutil.rmtree(work)


def report(work, summary):
    items = []
    for crate in CRATES:
        items += items_of(crate, os.path.join(work, "rustdoc"))
    copy = os.path.join(work, "tree")
    tracked = run(["git", "ls-files"], ROOT).stdout.split("\n")
    for name in tracked:
        if name and (ROOT / name).is_file():
            os.makedirs(os.path.dirname(os.path.join(copy, name)) or copy,
                        exist_ok=True)
            Path(copy, name).write_bytes((ROOT / name).read_bytes())
    mark(items, copy)
    checked = run(["cargo", "check", "--workspace", "--all-targets",
                   "--all-features", "--message-format=json",
                   "--target-dir", os.path.join(work, "check")],
                  copy, check=False)
    users = defaultdict(set)
    for line in checked.stdout.split("\n"):
        if not line.startswith("{"):
            continue
        message = json.loads(line)
        if message.get("reason") != "compiler-message":
            continue
        note = re.search(r"@@(\d+)", message["message"]["message"])
        primary = [s for s in message["message"]["spans"] if s["is_primary"]]
        if note and primary:
            item = items[int(note.group(1))]
            found = label(outermost(primary[0]), item["crate"])
            if found:
                users[item["number"]].add(found)
    derived = derive_paths()
    table = []
    for item in items:
        if item.get("skipped"):
            continue
        who = set(users[item["number"]])
        if item["crate"] == "data" and item["name"] in derived:
            who.add("derive")
        table.append((item, who))
    if summary:
        counts = defaultdict(lambda: defaultdict(int))
        for item, who in table:
            if who & set(PROGRAMS):
                kind = "programs"
            elif any(w.startswith("prodrome-") or w in ("law kit", "derive")
                     for w in who):
                kind = "other crates"
            elif who:
                kind = "tests only"
            else:
                kind = "nobody"
            counts[item["crate"]][kind] += 1
        columns = ["programs", "other crates", "tests only", "nobody"]
        print(f"{'crate':<10}" + "".join(f"{c:>14}" for c in columns))
        for crate in CRATES:
            print(f"{crate:<10}"
                  + "".join(f"{counts[crate][c]:>14}" for c in columns))
        return
    for item, who in table:
        ordered = [w for w in PROGRAMS if w in who]
        ordered += sorted(who - set(PROGRAMS))
        print("\t".join([item["crate"], f"{item['file']}:{item['line']}",
                         item["kind"], item["name"] or "?",
                         "hidden" if item["hidden"] else "",
                         ", ".join(ordered)]))


if __name__ == "__main__":
    main()
