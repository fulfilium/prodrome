//! One entry of a cache file (SPEC §6, "A cache file"): its bytes, and how a
//! reader finds and checks them.
//!
//! A record is, in order:
//!
//! | bytes | field |
//! |---|---|
//! | 4 | the mark `FF 6D 72 00` |
//! | 4 | the value's length `n`, big-endian |
//! | 32 | the digest of the function's name |
//! | 32 | the digest of the argument's name |
//! | `n` | the value: the canonical print of its literal (§2) |
//! | 32 | the check: the SHA-256 of every byte above |
//!
//! A print is UTF-8 text with no NUL byte, and `FF` is never a byte of
//! UTF-8, so the mark never occurs inside a value: a reader that loses its
//! place finds the next record at the next mark. A record whose check does
//! not hold is not an entry.

use sha2::{Digest as _, Sha256};

use crate::event::Hash;
use crate::memo::Key;

/// The first bytes of every record.
pub(super) const MARK: [u8; 4] = [0xFF, b'm', b'r', 0x00];

/// The bytes before the value: the mark, the length and the two names.
const HEAD: usize = 4 + 4 + 32 + 32;

/// The bytes of a record that a removal keeps: the mark and the length. A
/// removal writes zeros over the rest, so a dead record still frames, and
/// every reader agrees where the records of a file end.
pub(super) const KEPT: usize = 4 + 4;

/// The bytes after the value: the check.
const CHECK: usize = 32;

/// The longest value a record holds. A longer value is not kept, and a
/// length field above it is not a record's.
pub(super) const LONGEST: usize = 1 << 26;

/// What a reader learns from a record's first bytes, before it checks the
/// record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    /// A record's head, whose whole record lies within the bytes.
    Whole(Header),
    /// A dead record of this length: its mark and its length, and zeros
    /// for the rest of it.
    Dead(usize),
    /// A record's head, whose record runs past the end of the bytes.
    Short,
    /// No record starts here.
    Not,
}

/// A record's key, its length in bytes, and its check, as its head and its
/// last bytes read. Nothing is checked yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Header {
    pub names: Names,
    pub length: usize,
    pub check: [u8; 32],
}

/// A key as the two digests that a record holds. A holder indexes these,
/// and makes a [`Key`] only when a program asks for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct Names {
    function: [u8; 32],
    argument: [u8; 32],
}

impl Names {
    pub(super) fn of(key: &Key) -> Names {
        Names {
            function: key.function.digest(),
            argument: key.argument.digest(),
        }
    }

    pub(super) fn key(&self) -> Key {
        Key {
            function: Hash::of_digest(self.function),
            argument: Hash::of_digest(self.argument),
        }
    }
}

/// The record of `value` under `key` and its check, or `None` when the value
/// is longer than [`LONGEST`].
pub(super) fn encode(names: &Names, value: &[u8]) -> Option<(Vec<u8>, [u8; 32])> {
    let length = u32::try_from(value.len())
        .ok()
        .filter(|_| value.len() <= LONGEST)?;
    let mut record = Vec::with_capacity(HEAD + value.len() + CHECK);
    record.extend_from_slice(&MARK);
    record.extend_from_slice(&length.to_be_bytes());
    record.extend_from_slice(&names.function);
    record.extend_from_slice(&names.argument);
    record.extend_from_slice(value);
    let check: [u8; 32] = Sha256::digest(&record).into();
    record.extend_from_slice(&check);
    Some((record, check))
}

/// The frame of the record that starts at `bytes[0]`.
fn frame(bytes: &[u8]) -> Frame {
    let Some(rest) = bytes.strip_prefix(&MARK) else {
        // Fewer bytes than a mark, and all of them a mark's first bytes:
        // a record cut short.
        return if MARK.starts_with(bytes) {
            Frame::Short
        } else {
            Frame::Not
        };
    };
    let Some((length, rest)) = rest.split_first_chunk::<4>() else {
        return Frame::Short;
    };
    let length = u32::from_be_bytes(*length) as usize;
    if length > LONGEST {
        return Frame::Not;
    }
    let Some((function, rest)) = rest.split_first_chunk::<32>() else {
        return Frame::Short;
    };
    let Some((argument, rest)) = rest.split_first_chunk::<32>() else {
        return Frame::Short;
    };
    let Some(check) = rest.get(length..).and_then(<[u8]>::first_chunk::<CHECK>) else {
        return Frame::Short;
    };
    let full = HEAD + length + CHECK;
    // A print is never empty and holds no NUL, so a record whose value is
    // zeros is dead.
    if bytes
        .get(KEPT..full)
        .is_some_and(|rest| rest.iter().all(|byte| *byte == 0))
    {
        return Frame::Dead(full);
    }
    Frame::Whole(Header {
        names: Names {
            function: *function,
            argument: *argument,
        },
        length: full,
        check: *check,
    })
}

/// The value of `record` when `record` is exactly one whole record, its
/// check holds, and its key is `names`.
pub(super) fn value<'r>(record: &'r [u8], names: &Names) -> Option<&'r [u8]> {
    let Frame::Whole(header) = frame(record) else {
        return None;
    };
    let (body, _) = record.split_at_checked(header.length - CHECK)?;
    let holds = header.length == record.len()
        && header.names == *names
        && <[u8; 32]>::from(Sha256::digest(body)) == header.check;
    holds.then_some(body.get(HEAD..)?)
}

/// What a reader found in a run of bytes.
pub(super) struct Scan {
    /// Every whole record, with its offset, in order.
    pub found: Vec<(usize, Header)>,
    /// The end of the last record, whole or dead, or 0 for none.
    pub end: usize,
    /// The start of that record, or 0 for none.
    pub last: usize,
}

/// Every record in `bytes`. A reader that finds no record where one should
/// start goes on at the next mark.
pub(super) fn records(bytes: &[u8]) -> Scan {
    let mut scan = Scan {
        found: Vec::new(),
        end: 0,
        last: 0,
    };
    let mut at = 0;
    while at < bytes.len() {
        let length = match frame(&bytes[at..]) {
            Frame::Whole(header) => {
                scan.found.push((at, header));
                header.length
            }
            Frame::Dead(length) => length,
            Frame::Short | Frame::Not => match next_mark(bytes, at + 1) {
                Some(next) => {
                    at = next;
                    continue;
                }
                None => break,
            },
        };
        scan.last = at;
        at += length;
        scan.end = at;
    }
    scan
}

/// Whether `bytes` are exactly one record, whole or dead.
pub(super) fn one(bytes: &[u8]) -> bool {
    match frame(bytes) {
        Frame::Whole(Header { length, .. }) | Frame::Dead(length) => length == bytes.len(),
        Frame::Short | Frame::Not => false,
    }
}

/// Where zeros over the record of `length` that starts at `at` in `bytes`
/// may end: the record's end, or the next mark before it, so that a length
/// that was never checked reaches no record after it.
pub(super) fn reach(bytes: &[u8], at: usize, length: usize) -> usize {
    next_mark(bytes, at + 1).map_or(at + length, |next| next.min(at + length))
}

/// Where the next mark at or after `from` starts in `bytes`.
fn next_mark(bytes: &[u8], from: usize) -> Option<usize> {
    let rest = bytes.get(from..)?;
    memchr::memmem::find(rest, &MARK).map(|at| from + at)
}
