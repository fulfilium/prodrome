//! The count of a store's holds: the file `root/.writes`.
//!
//! The file holds a number that every hold of the store's lock increments
//! when it lets the lock go, and a random tag that a new file draws. A
//! handle that has read the store keeps the count that it read before its
//! last listing of `objects/`, or the count that its own hold wrote. A read
//! by that handle then reads the file alone, and lists `objects/` only when
//! the file is absent, does not parse, or holds another count: then another
//! handle held the lock since.
//!
//! A hold removes the file before its first change of `objects/`, and
//! writes it when it lets the lock go. So a reader that finds the count it
//! keeps read it before the hold changed anything. A program that changes
//! `objects/` by another door (a checkout, a copy of files, an older
//! binary) does not touch the file, so a handle sees that change at the
//! next hold of the lock by any handle, as the record of heads does (SPEC
//! law 62). The file is derived and local to one machine, and the store's
//! `.gitignore` keeps it out of version control.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::literal::ProdromeError;

/// The file's name in the store's root.
pub(super) const FILE: &str = ".writes";

/// A count of holds, under the tag that the file drew when it was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Count {
    tag: u64,
    holds: u64,
}

impl Count {
    /// The count that follows `self`, or a new tag where there is no
    /// count.
    pub(super) fn after(count: Option<Count>, tag: impl FnOnce() -> u64) -> Count {
        match count {
            Some(Count { tag, holds }) => Count {
                tag,
                holds: holds.wrapping_add(1),
            },
            None => Count {
                tag: tag(),
                holds: 0,
            },
        }
    }

    fn print(self) -> String {
        format!("{:016x} {}", self.tag, self.holds)
    }

    fn parse(text: &str) -> Option<Count> {
        let (tag, holds) = text.split_once(' ')?;
        let count = Count {
            tag: u64::from_str_radix(tag, 16).ok()?,
            holds: holds.parse().ok()?,
        };
        (count.print() == text).then_some(count)
    }
}

/// The count in `root`, if the file is there and parses.
pub(super) fn read(root: &Path) -> Option<Count> {
    Count::parse(&fs::read_to_string(root.join(FILE)).ok()?)
}

/// Remove the file before a hold changes `objects/`. No sync: a power loss
/// ends every handle, and a new handle lists `objects/` whatever the file
/// holds.
pub(super) fn remove(root: &Path) -> Result<(), ProdromeError> {
    let path = root.join(FILE);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ProdromeError::io(&path, &error)),
    }
}

/// `count` as the file of `root`: any file there unlinked, and the file
/// created anew. A file cut short does not parse, so a reader then lists.
pub(super) fn write(root: &Path, count: Count) -> Result<(), ProdromeError> {
    remove(root)?;
    let path = root.join(FILE);
    File::options()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut file| file.write_all(count.print().as_bytes()))
        .map_err(|error| ProdromeError::io(&path, &error))
}

#[cfg(test)]
mod tests {
    use super::Count;

    /// The count's bytes are its print, and no prefix of a print and no
    /// other spelling of a count parses.
    #[test]
    fn a_count_is_its_print_and_nothing_else_reads() {
        let count = Count::after(Some(Count::after(None, || 0xab)), || 0);
        assert_eq!(count.print(), "00000000000000ab 1");
        assert_eq!(Count::parse(&count.print()), Some(count));
        for cut in 0..count.print().len() {
            assert_eq!(Count::parse(&count.print()[..cut]), None);
        }
        for other in ["ab 1", "00000000000000ab 01", "00000000000000ab 1\n"] {
            assert_eq!(Count::parse(other), None, "{other}");
        }
    }
}
