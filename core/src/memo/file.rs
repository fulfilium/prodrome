//! A holder whose pairs are in one file (design §6.2; SPEC §6, "A cache
//! file").
//!
//! The file is a header and then a run of records, each one
//! pair, each carrying the SHA-256 of its own bytes. A holder keeps an index
//! in memory from each key to where its records lie, and checks a record
//! each time it reads one. So a record that a stop cut short, that the disk
//! damaged, or that another program wrote in another form is not a value:
//! its key misses (SPEC law 52).
//!
//! THE ORDER OF WRITES. A holder puts each new record in a buffer in memory.
//! It writes the buffer when the buffer is full, at [`Holder::settle`], and
//! when the holder is dropped. To write, it takes an exclusive `flock` on
//! the file, reads the records that other holders appended, truncates a
//! record cut short at the end, writes zeros over the records of the pairs
//! it removed, and appends the buffer in one write. A stop before the
//! append leaves the file as it was, with some removed records zeroed. A
//! stop during the append leaves whole records and one record cut short at
//! the end, which is no entry. A record that the disk did not keep, or kept
//! in part, is no entry. So a stop at any point leaves a file that holds a
//! subset of the pairs that were written (SPEC law 54).
//!
//! TWO PROCESSES that write one file each take the lock for each write. A
//! holder reads a record without a lock and checks it instead. When a
//! record that its index names does not check, the holder reads the file
//! again under a shared lock.
//!
//! THE BOUND. A removed record is dead. When the dead bytes are more than
//! the live bytes and more than [`SLACK`], the holder COMPACTS the file in
//! place. Under the exclusive lock it raises the generation in the header,
//! writes the live records that check one after the other from the header,
//! truncates the rest, and syncs. So a file is at most twice its live
//! records, the slack and one buffer long, and a policy that bounds the
//! keys bounds the file ([`super::Cache`], SPEC law 58). Another process
//! sees the new generation and reads the file again. The file keeps its
//! inode, so every process that holds it open reads the compacted file.
//!
//! FORGETTING ([`Holder::forget`]) writes zeros over every record of each
//! doomed key, syncs, and then compacts and syncs. Then no record of a
//! doomed key, and no damaged remnant of any record, is left in the file
//! (SPEC law 55).

mod record;

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::marker::PhantomData;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;

use rustix::fs::{flock, FlockOperation};

use self::record::{Header, Names};
use super::cache::{Finding, Key, Lookup};
use super::holder::Holder;
use crate::literal::codec::Literal;
use crate::literal::{parse_literal, print_literal, Open, ProdromeError};

/// The first eight bytes of a cache file. The next eight bytes are its
/// generation, big-endian, which each compaction raises by one.
const MAGIC: [u8; 8] = *b"\xffprodmc1";

/// The length of the header: the magic and the generation.
const HEADER: u64 = 16;

/// The dead bytes that a file may hold however few its live bytes are.
pub const SLACK: u64 = 1 << 16;

/// The length of the buffer of new records at which a holder writes it.
pub const BATCH: usize = 1 << 18;

/// The pairs of a cache, in one file. A value is kept as the canonical
/// print of its literal (§2), so a value type must be a [`Literal`].
#[derive(Debug)]
pub struct File<V> {
    path: PathBuf,
    handle: fs::File,
    index: HashMap<Names, Vec<Slot>>,
    /// How far this holder read the file.
    seen: Seen,
    /// The length of the written records that the index names.
    live: u64,
    /// New records, not yet written, one after the other.
    pending: Vec<u8>,
    /// The keys of the records in `pending`, in order.
    waiting: Vec<Names>,
    /// The pairs removed whose records are not yet zeroed, each with the
    /// places of its records in the file.
    removed: HashMap<Pair, Vec<(u64, usize)>>,
    /// The writes that failed since the last `settle`.
    failed: Vec<ProdromeError>,
    value: PhantomData<fn() -> V>,
}

/// A key and the check of one of its records, which together name one pair.
type Pair = (Names, [u8; 32]);

/// How far a holder read its file, and at which generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seen {
    /// The end of the last whole record read.
    end: u64,
    generation: u64,
}

/// Where one record of a key is, and its check. Two records of one key
/// hold one value exactly when their checks are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    place: Place,
    length: usize,
    check: [u8; 32],
}

/// Where a record's bytes are. A written record is older than a pending
/// one, so places order as the records arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    /// At this offset in the file.
    Written(u64),
    /// At this offset in the buffer of new records.
    Pending(usize),
}

/// What reading a key's records found.
enum Read<V> {
    /// Every record that the index names checked. Their distinct values.
    Checked(Vec<V>),
    /// A record that the index names did not check, so the file may have
    /// changed under this holder. The values of the records that checked.
    Moved(Vec<V>),
}

impl<V> File<V> {
    /// The cache file at `path`. The holder makes an empty cache file when
    /// no file is there.
    ///
    /// # Errors
    ///
    /// The file cannot be opened, locked or read, or it is not a cache
    /// file. A record that does not check is no error: it is no entry.
    pub fn open(path: impl Into<PathBuf>) -> Result<File<V>, ProdromeError> {
        let path = path.into();
        let handle = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| ProdromeError::io(&path, &error))?;
        let mut file = File {
            path,
            handle,
            index: HashMap::new(),
            seen: Seen {
                end: HEADER,
                generation: 0,
            },
            live: 0,
            pending: Vec::new(),
            waiting: Vec::new(),
            removed: HashMap::new(),
            failed: Vec::new(),
            value: PhantomData,
        };
        file.locked(FlockOperation::LockExclusive, |file| {
            file.begin()?;
            file.read_from(HEADER)
        })?;
        Ok(file)
    }

    /// `work`, under the lock that `operation` takes on the file.
    fn locked<T>(
        &mut self,
        operation: FlockOperation,
        work: impl FnOnce(&mut Self) -> Result<T, ProdromeError>,
    ) -> Result<T, ProdromeError> {
        flock(&self.handle, operation)
            .map_err(|error| ProdromeError::io(&self.path, &std::io::Error::from(error)))?;
        let done = work(self);
        // If the unlock fails, the lock stays until the handle closes. That
        // delays another process and changes no value.
        let _unlocked = flock(&self.handle, FlockOperation::Unlock);
        done
    }

    /// Writes the header of an empty file, and refuses a file with another
    /// header. The caller holds the exclusive lock.
    fn begin(&mut self) -> Result<(), ProdromeError> {
        if self.length()? == 0 {
            let mut header = MAGIC.to_vec();
            header.extend_from_slice(&0_u64.to_be_bytes());
            return self
                .handle
                .write_all_at(&header, 0)
                .and_then(|()| self.handle.sync_data())
                .map_err(|error| ProdromeError::io(&self.path, &error));
        }
        let mut magic = [0; 8];
        match self.handle.read_exact_at(&mut magic, 0) {
            Ok(()) if magic == MAGIC => Ok(()),
            Ok(()) | Err(_) => Err(ProdromeError::Store(format!(
                "{} is not a cache file",
                self.path.display()
            ))),
        }
    }

    fn length(&self) -> Result<u64, ProdromeError> {
        self.handle
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|error| ProdromeError::io(&self.path, &error))
    }

    fn generation(&self) -> Result<u64, ProdromeError> {
        let mut word = [0; 8];
        self.handle
            .read_exact_at(&mut word, 8)
            .map_err(|error| ProdromeError::io(&self.path, &error))?;
        Ok(u64::from_be_bytes(word))
    }

    /// The bytes of the file from `from` to its end.
    fn tail(&self, from: u64) -> Result<Vec<u8>, ProdromeError> {
        let size = usize::try_from(self.length()?.saturating_sub(from))
            .map_err(|_| ProdromeError::Store(format!("{} is too large", self.path.display())))?;
        let mut bytes = vec![0; size];
        self.handle
            .read_exact_at(&mut bytes, from)
            .map_err(|error| ProdromeError::io(&self.path, &error))?;
        Ok(bytes)
    }

    /// Reads what changed in the file since this holder last read it: the
    /// records appended after its end, or the whole file again when another
    /// holder compacted it. The caller holds a lock.
    fn catch_up(&mut self) -> Result<(), ProdromeError> {
        if self.generation()? != self.seen.generation || self.length()? < self.seen.end {
            return self.reread();
        }
        self.read_from(self.seen.end)
    }

    /// Reads the whole file again. The pending records stay. The caller
    /// holds a lock.
    fn reread(&mut self) -> Result<(), ProdromeError> {
        for slots in self.index.values_mut() {
            slots.retain(|slot| matches!(slot.place, Place::Pending(_)));
        }
        self.index.retain(|_, slots| !slots.is_empty());
        self.live = 0;
        for places in self.removed.values_mut() {
            places.clear();
        }
        self.read_from(HEADER)
    }

    /// Indexes every record from `from` to the end of the file, and moves
    /// `seen` to the end of the last whole record and to the file's
    /// generation. The caller holds a lock.
    fn read_from(&mut self, from: u64) -> Result<(), ProdromeError> {
        let (found, end) = record::records(&self.tail(from)?);
        for (at, header) in found {
            self.index_slot(header, from + at as u64);
        }
        self.seen = Seen {
            end: from + end as u64,
            generation: self.generation()?,
        };
        Ok(())
    }

    /// Indexes the record at `at`. A record of a removed pair is listed to
    /// be zeroed, and a second record of one pair is dead.
    fn index_slot(&mut self, header: Header, at: u64) {
        let pair = (header.names, header.check);
        if let Some(places) = self.removed.get_mut(&pair) {
            places.push((at, header.length));
            return;
        }
        let (names, check) = pair;
        let slots = self.index.entry(names).or_default();
        if slots.iter().any(|slot| slot.check == check) {
            return;
        }
        self.live += header.length as u64;
        slots.push(Slot {
            place: Place::Written(at),
            length: header.length,
            check,
        });
    }

    /// The bytes of the record in `slot` as they are now. None where the
    /// file is shorter.
    fn bytes(&self, slot: Slot) -> Vec<u8> {
        match slot.place {
            Place::Written(at) => {
                let mut bytes = vec![0; slot.length];
                match self.handle.read_exact_at(&mut bytes, at) {
                    Ok(()) => bytes,
                    Err(_) => Vec::new(),
                }
            }
            Place::Pending(at) => self
                .pending
                .get(at..at + slot.length)
                .map(<[u8]>::to_vec)
                .unwrap_or_default(),
        }
    }

    /// Writes zeros over the records of the removed pairs, each after its
    /// bytes check as that pair's record, and then writes the pending
    /// records after the last whole record. The caller holds the exclusive
    /// lock.
    fn write(&mut self) -> Result<(), ProdromeError> {
        self.catch_up()?;
        for ((names, check), places) in std::mem::take(&mut self.removed) {
            for (at, length) in places {
                let bytes = self.bytes(Slot {
                    place: Place::Written(at),
                    length,
                    check,
                });
                if record::value(&bytes, &names).is_some() && bytes.ends_with(&check) {
                    self.handle
                        .write_all_at(&vec![0; length], at)
                        .map_err(|error| ProdromeError::io(&self.path, &error))?;
                }
            }
        }
        if self.pending.is_empty() {
            return Ok(());
        }
        let written = self
            .handle
            .set_len(self.seen.end)
            .and_then(|()| self.handle.write_all_at(&self.pending, self.seen.end))
            .map_err(|error| ProdromeError::io(&self.path, &error));
        let base = self.seen.end;
        for names in std::mem::take(&mut self.waiting) {
            let Some(slots) = self.index.get_mut(&names) else {
                continue;
            };
            for slot in slots.iter_mut() {
                if let (Place::Pending(at), Ok(())) = (slot.place, &written) {
                    slot.place = Place::Written(base + at as u64);
                    self.live += slot.length as u64;
                }
            }
            slots.retain(|slot| matches!(slot.place, Place::Written(_)));
            if slots.is_empty() {
                self.index.remove(&names);
            }
        }
        if written.is_ok() {
            self.seen.end += self.pending.len() as u64;
        }
        self.pending.clear();
        written
    }

    /// Writes what is pending under the exclusive lock.
    fn flush(&mut self) -> Result<(), ProdromeError> {
        if self.pending.is_empty() && self.removed.is_empty() {
            return Ok(());
        }
        self.locked(FlockOperation::LockExclusive, File::write)
    }

    /// Writes the records of every key that the index names and `doomed`
    /// does not, one after the other from the header and under a raised
    /// generation; truncates the rest; and syncs. A record that does not
    /// check is not written. The caller holds the exclusive lock, and
    /// nothing is pending.
    fn rewrite(&mut self, doomed: &BTreeSet<Key>) -> Result<(), ProdromeError> {
        let generation = self.seen.generation.wrapping_add(1);
        self.handle
            .write_all_at(&generation.to_be_bytes(), 8)
            .map_err(|error| ProdromeError::io(&self.path, &error))?;
        let old = self.tail(HEADER)?;
        let mut slots: Vec<(u64, Names, Slot)> = Vec::new();
        let doomed: BTreeSet<Names> = doomed.iter().map(Names::of).collect();
        for (names, held) in &self.index {
            for slot in held.iter().filter(|_| !doomed.contains(names)) {
                if let Place::Written(at) = slot.place {
                    slots.push((at, *names, *slot));
                }
            }
        }
        slots.sort_by_key(|(at, _, _)| *at);
        let mut new = Vec::with_capacity(old.len());
        let mut index: HashMap<Names, Vec<Slot>> = HashMap::new();
        for (at, names, slot) in slots {
            let bytes = usize::try_from(at - HEADER)
                .ok()
                .and_then(|start| old.get(start..start + slot.length))
                .filter(|bytes| record::value(bytes, &names).is_some());
            if let Some(bytes) = bytes {
                index.entry(names).or_default().push(Slot {
                    place: Place::Written(HEADER + new.len() as u64),
                    ..slot
                });
                new.extend_from_slice(bytes);
            }
        }
        let end = HEADER + new.len() as u64;
        self.handle
            .write_all_at(&new, HEADER)
            .and_then(|()| self.handle.set_len(end))
            .and_then(|()| self.handle.sync_data())
            .map_err(|error| ProdromeError::io(&self.path, &error))?;
        self.index = index;
        self.live = new.len() as u64;
        self.removed.clear();
        self.seen = Seen { end, generation };
        Ok(())
    }

    /// Compacts the file when its dead bytes are more than its live bytes
    /// and more than [`SLACK`].
    fn compact_if_due(&mut self) {
        let dead = self.seen.end - HEADER - self.live;
        if dead <= self.live.max(SLACK) {
            return;
        }
        let compacted = self.locked(FlockOperation::LockExclusive, |file| {
            file.write()?;
            file.rewrite(&BTreeSet::new())
        });
        if let Err(error) = compacted {
            self.failed.push(error);
        }
    }
}

impl<V: Literal + Clone + Eq> File<V> {
    /// The values of `key`'s records. A record that does not check, or
    /// whose value does not read as a `V`, leaves the index.
    fn read(&mut self, names: Names) -> Read<V> {
        let slots = self.index.remove(&names).unwrap_or_default();
        let mut values: Vec<V> = Vec::new();
        let mut kept = Vec::new();
        let mut moved = false;
        for slot in slots {
            let bytes = self.bytes(slot);
            let print = record::value(&bytes, &names);
            moved |= print.is_none();
            match print.and_then(decode::<V>) {
                Some(value) => {
                    kept.push(slot);
                    if !values.contains(&value) {
                        values.push(value);
                    }
                }
                None => {
                    if let Place::Written(_) = slot.place {
                        self.live -= slot.length as u64;
                    }
                }
            }
        }
        if !kept.is_empty() {
            self.index.insert(names, kept);
        }
        if moved {
            Read::Moved(values)
        } else {
            Read::Checked(values)
        }
    }
}

impl<V: Literal + Clone + Eq> Holder<V> for File<V> {
    fn get(&mut self, key: &Key) -> Lookup<V> {
        let names = Names::of(key);
        if !self.index.contains_key(&names) {
            return Lookup::Miss;
        }
        match self.read(names) {
            Read::Checked(values) => values.into(),
            Read::Moved(_) => {
                // Another holder may have compacted or erased the file: the
                // holder reads it again, and the key where it is now.
                if let Err(error) = self.locked(FlockOperation::LockShared, File::catch_up) {
                    self.failed.push(error);
                }
                match self.read(names) {
                    Read::Checked(values) | Read::Moved(values) => values.into(),
                }
            }
        }
    }

    fn add(&mut self, key: Key, value: V) -> Result<(), Finding> {
        let print = print_literal(&value.to_value());
        // A value too long for a record is not kept: the pair is dropped.
        let names = Names::of(&key);
        let Some((record, check)) = record::encode(&names, print.as_bytes()) else {
            return Ok(());
        };
        let slots = self.index.entry(names).or_default();
        if slots.iter().any(|slot| slot.check == check) {
            return Ok(());
        }
        let second = slots.len() == 1;
        slots.push(Slot {
            place: Place::Pending(self.pending.len()),
            length: record.len(),
            check,
        });
        self.pending.extend_from_slice(&record);
        self.waiting.push(names);
        if self.pending.len() >= BATCH {
            if let Err(error) = self.flush() {
                self.failed.push(error);
            }
        }
        if second {
            Err(Finding { key })
        } else {
            Ok(())
        }
    }

    fn remove(&mut self, key: &Key) -> bool {
        let names = Names::of(key);
        let Some(slots) = self.index.remove(&names) else {
            return false;
        };
        for slot in slots {
            match slot.place {
                Place::Written(at) => {
                    self.live -= slot.length as u64;
                    self.removed
                        .entry((names, slot.check))
                        .or_default()
                        .push((at, slot.length));
                }
                Place::Pending(at) => {
                    if let Some(bytes) = self.pending.get_mut(at..at + slot.length) {
                        bytes.fill(0);
                    }
                }
            }
        }
        self.compact_if_due();
        true
    }

    fn forget(&mut self, doomed: &BTreeSet<Key>) -> Result<(), ProdromeError> {
        for key in doomed {
            Holder::<V>::remove(self, key);
        }
        self.locked(FlockOperation::LockExclusive, |file| {
            file.write()?;
            // Zeros over every record of a doomed key that the file holds,
            // a record this holder never indexed included.
            let names: BTreeSet<Names> = doomed.iter().map(Names::of).collect();
            let (found, _) = record::records(&file.tail(HEADER)?);
            for (at, header) in found
                .iter()
                .filter(|(_, header)| names.contains(&header.names))
            {
                file.handle
                    .write_all_at(&vec![0; header.length], HEADER + *at as u64)
                    .map_err(|error| ProdromeError::io(&file.path, &error))?;
            }
            file.handle
                .sync_data()
                .map_err(|error| ProdromeError::io(&file.path, &error))?;
            file.rewrite(doomed)
        })
    }

    fn keys(&self) -> impl Iterator<Item = Key> + '_ {
        let mut keys: Vec<(Place, &Names)> = self
            .index
            .iter()
            .filter_map(|(names, slots)| {
                slots
                    .iter()
                    .map(|slot| slot.place)
                    .min()
                    .map(|at| (at, names))
            })
            .collect();
        keys.sort();
        keys.into_iter().map(|(_, names)| names.key())
    }

    fn len(&self) -> usize {
        self.index.len()
    }

    fn settle(&mut self) -> Result<(), ProdromeError> {
        let flushed = self.flush();
        let synced = self
            .handle
            .sync_data()
            .map_err(|error| ProdromeError::io(&self.path, &error));
        let mut failed = std::mem::take(&mut self.failed);
        failed.extend(flushed.err());
        failed.extend(synced.err());
        failed.into_iter().next().map_or(Ok(()), Err)
    }
}

/// A holder that is dropped writes what is pending. If the write fails, the
/// pending pairs are dropped, and no reader needs them.
impl<V> Drop for File<V> {
    fn drop(&mut self) {
        let _written = self.flush();
    }
}

/// The value whose print is `print`, or none for bytes that do not read as
/// a `V`.
fn decode<V: Literal>(print: &[u8]) -> Option<V> {
    let text = std::str::from_utf8(print).ok()?;
    V::from_value(&parse_literal(text, &Open).ok()?).ok()
}
