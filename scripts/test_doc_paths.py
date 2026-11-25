#!/usr/bin/env python3
"""Check `scripts/doc_paths.py`: which spans it takes for a path, and which
it accepts as a name. Run it from the repository root:

    python3 scripts/test_doc_paths.py
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import doc_paths  # noqa: E402

FILES = {"core/src/lib.rs", "tags/src/alias.rs", "docs/guide.md", "SPEC.md"}
DIRECTORIES = {"core", "core/src", "tags", "tags/src", "docs"}


class Names(unittest.TestCase):
    def test_a_file_from_the_root(self):
        self.assertTrue(doc_paths.names("SPEC.md", "README.md", FILES))
        self.assertFalse(doc_paths.names("MISSING.md", "README.md", FILES))

    def test_a_file_from_the_directory_of_the_document(self):
        self.assertTrue(doc_paths.names("guide.md", "docs/README.md", FILES))
        self.assertTrue(doc_paths.names("../SPEC.md", "docs/README.md", FILES))
        self.assertFalse(doc_paths.names("nothing.md", "docs/README.md", FILES))

    def test_a_file_inside_the_src_of_a_crate(self):
        self.assertTrue(doc_paths.names("tags/alias.rs", "docs/note.md", FILES))

    def test_a_file_by_the_end_of_its_path(self):
        self.assertTrue(doc_paths.names("lib.rs", "docs/note.md", FILES))
        self.assertFalse(doc_paths.names("other/lib.rs", "docs/note.md", FILES))

    def test_a_directory(self):
        self.assertTrue(doc_paths.names("core/src/", "README.md", DIRECTORIES))
        self.assertFalse(doc_paths.names("core/tests/", "README.md", DIRECTORIES))

    def test_a_path_that_git_does_not_track(self):
        self.assertTrue(doc_paths.names("target/", "README.md", DIRECTORIES))

    def test_a_path_as_it_was(self):
        self.assertTrue(doc_paths.names("core/src/store.rs", "README.md", FILES))


class Spans(unittest.TestCase):
    def test_a_directory_ends_in_a_slash(self):
        self.assertTrue(doc_paths.DIRECTORY.fullmatch("core/src/"))
        self.assertIsNone(doc_paths.DIRECTORY.fullmatch("core/src"))
        self.assertIsNone(doc_paths.DIRECTORY.fullmatch("a b/"))

    def test_a_file_ends_in_an_extension(self):
        self.assertTrue(doc_paths.FILE.fullmatch("core/src/lib.rs"))
        self.assertIsNone(doc_paths.FILE.fullmatch("core/src/"))


class Tables(unittest.TestCase):
    def test_no_historical_path_exists_and_each_destination_does(self):
        files, directories = doc_paths.tracked()
        self.assertEqual(list(doc_paths.stale(files, directories)), [])


if __name__ == "__main__":
    unittest.main()
