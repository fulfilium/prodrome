//! The mark of a store's last hold: the file `root/.writes`.
//!
//! Every hold of the store's lock removes the file when it takes the lock,
//! and writes a fresh random mark when it lets the lock go. A handle that
//! has read the store keeps the mark that it read before its last listing
//! of `objects/`, or the mark that its own hold wrote. A read by that
//! handle then reads the file alone, and lists `objects/` only when the
//! file is absent, does not parse, or holds another mark: then another
//! handle held the lock since.
//!
//! A mark is compared for equality alone. It is fresh for each hold and
//! has a fixed width, so a file cut short holds no mark, and a file put
//! back to an older mark (a checkout, a backup) holds a mark that no handle
//! keeps after a later hold. A hold removes the file before it changes
//! `objects/`, so a reader that finds the mark it keeps read it before the
//! hold changed anything. A program that changes `objects/` by another door
//! (a checkout, a copy of files, an older binary) does not touch the file,
//! so a handle sees that change at the next hold of the lock that lists
//! `objects/`, as the record of heads does (SPEC law 62): every hold of a
//! store opened as shared, and `record_heads()` and `fsck` of a store
//! opened as alone. The file is local to one
//! machine, and the store's `.gitignore` keeps it out of version control.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::literal::ProdromeError;

/// The file's name in the store's root.
pub(super) const FILE: &str = ".writes";

/// The mark of one hold: 128 random bits from the system, printed as 32
/// lowercase hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Mark(u64, u64);

impl Mark {
    /// A fresh mark, drawn from the system's random source: `getrandom` on
    /// Linux, `/dev/urandom` on another unix. Two processes forked from one
    /// parent draw different marks, which the standard library's hash keys,
    /// copied by the fork, would not. None where the source cannot be read;
    /// a hold then keeps no mark, and every handle lists.
    pub(super) fn fresh() -> Option<Mark> {
        let mut bytes = [0u8; 16];
        fill(&mut bytes)?;
        let (high, low) = bytes.split_at(8);
        Some(Mark(
            u64::from_le_bytes(high.try_into().ok()?),
            u64::from_le_bytes(low.try_into().ok()?),
        ))
    }

    fn print(self) -> String {
        format!("{:016x}{:016x}", self.0, self.1)
    }

    fn parse(text: &str) -> Option<Mark> {
        let digits = |part: &str| {
            (part.len() == 16 && part.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
                .then(|| u64::from_str_radix(part, 16).ok())
                .flatten()
        };
        let (high, low) = (text.get(..16)?, text.get(16..)?);
        Some(Mark(digits(high)?, digits(low)?))
    }
}

/// `bytes` filled from the system's random source on Linux.
#[cfg(target_os = "linux")]
fn fill(bytes: &mut [u8]) -> Option<()> {
    let mut filled = 0;
    while filled < bytes.len() {
        match rustix::rand::getrandom(&mut bytes[filled..], rustix::rand::GetRandomFlags::empty()) {
            Ok(read) => filled += read,
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return None,
        }
    }
    Some(())
}

/// `bytes` filled from `/dev/urandom` on another unix.
#[cfg(all(unix, not(target_os = "linux")))]
fn fill(bytes: &mut [u8]) -> Option<()> {
    use std::io::Read;
    File::open("/dev/urandom").ok()?.read_exact(bytes).ok()
}

/// No source off unix, where no store is written.
#[cfg(not(unix))]
fn fill(_bytes: &mut [u8]) -> Option<()> {
    None
}

/// The mark in `root`, if the file is there and parses.
pub(super) fn read(root: &Path) -> Option<Mark> {
    Mark::parse(&fs::read_to_string(root.join(FILE)).ok()?)
}

/// Remove the file when a hold takes the lock. No sync: a power loss ends
/// every handle, and a new handle lists `objects/` whatever the file holds.
pub(super) fn remove(root: &Path) -> Result<(), ProdromeError> {
    let path = root.join(FILE);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ProdromeError::io(&path, &error)),
    }
}

/// `mark` as the file of `root`: any file there unlinked, and the file
/// created anew and written, with no sync.
pub(super) fn write(root: &Path, mark: Mark) -> Result<(), ProdromeError> {
    remove(root)?;
    let path = root.join(FILE);
    File::options()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut file| file.write_all(mark.print().as_bytes()))
        .map_err(|error| ProdromeError::io(&path, &error))
}

#[cfg(test)]
mod tests {
    use super::Mark;

    /// A mark's bytes are its print, and no prefix of a print and no other
    /// spelling of a mark parses.
    #[test]
    fn two_fresh_marks_differ() {
        let (one, two) = (Mark::fresh(), Mark::fresh());
        assert!(one.is_some() && one != two);
    }

    #[test]
    fn a_mark_is_its_print_and_nothing_else_reads() {
        let mark = Mark(0xab, u64::MAX);
        let print = mark.print();
        assert_eq!(print, "00000000000000abffffffffffffffff");
        assert_eq!(Mark::parse(&print), Some(mark));
        for cut in 0..print.len() {
            assert_eq!(Mark::parse(&print[..cut]), None, "cut at {cut}");
        }
        for other in [
            format!("{print}\n"),
            print.to_uppercase(),
            format!("+{}", &print[1..]),
            format!("{print}0"),
        ] {
            assert_eq!(Mark::parse(&other), None, "{other}");
        }
    }
}
