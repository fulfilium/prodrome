#!/usr/bin/env python3
"""Check `scripts/edges.py` on the fixture under `scripts/edges/src`.

The fixture holds one use of each form that the script must read, and one
of each form that it must not read. Run it from the repository root:

    python3 scripts/test_edges.py
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import edges  # noqa: E402

SRC = Path(__file__).parent / "edges" / "src"


class Edges(unittest.TestCase):
    def found(self, *chosen):
        found, _ = edges.edges(SRC, chosen)
        return sorted((p.relative_to(SRC).as_posix(), line, root, name) for p, line, root, name in found)

    def test_each_form_of_a_use_outside_the_set(self):
        self.assertEqual(
            self.found("low"),
            [
                ("low.rs", 3, "crate", "high"),
                ("low.rs", 4, "crate", "other"),
                ("low.rs", 8, "crate", "high"),
                ("low.rs", 13, "crate", "high"),
                ("low.rs", 14, "crate", "other"),
                ("low.rs", 25, "crate", "high"),
                ("low.rs", 26, "prodrome", "event"),
                ("low/inner.rs", 2, "crate", "other"),
            ],
        )

    def test_a_set_that_holds_every_module_finds_only_the_facade(self):
        self.assertEqual(self.found(), [("low.rs", 26, "prodrome", "event")])

    def test_a_module_that_names_nothing_outside_passes(self):
        self.assertEqual(self.found("high"), [])


if __name__ == "__main__":
    unittest.main()
