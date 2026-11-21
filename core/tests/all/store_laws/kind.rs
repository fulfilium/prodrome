//! The factory of the suite: each kind of store that the laws run over, and
//! what a store of record must provide to be in it.
//!
//! A store of record is a store that holds the lock that every writer
//! honours, so that it alone can make a [`prodrome::store::Decision`]. The
//! one kind in this crate is the store of files ([`EventStore`] over a
//! directory). A replica is any value that implements [`Replica`]. The
//! suite runs a law of replicas over every store of record and over
//! [`MemoryStore`].
//!
//! A kind of store of record joins the suite when it implements
//! [`StoreOfRecord`] and has one line in [`stores_of_record`]. Then every
//! law runs over it, and every law of replicas runs over it too.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use prodrome::dag::{Dag, Finding, Heads};
use prodrome::event::{canonical_envelope, seal_hash, Envelope, Hash};
use prodrome::genesis::mk_genesis;
use prodrome::literal::ProdromeError;
use prodrome::policy::Untrusted;
use prodrome::schema::Schema;
use prodrome::store::{Decision, EventStore, Husking, MemoryStore, Replica, Sender, Stopped};
use sha2::{Digest, Sha256};

/// The number in the name of each new directory, so that no two stores of a
/// run share one.
static NEXT: AtomicU64 = AtomicU64::new(0);

fn next() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A directory for the stores of one case, empty when the case begins.
/// Dropping it removes the directory and every store in it.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// A new scratch directory. A run that was killed, or an earlier
    /// process with the same id, can have left one under the same name, so
    /// it is removed first.
    pub(crate) fn new() -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "prodrome-store-laws-{}-{}",
            std::process::id(),
            next()
        ));
        let _ = fs::remove_dir_all(&root);
        Scratch(root)
    }

    /// A path in the scratch directory that no other store uses.
    pub(crate) fn fresh(&self) -> PathBuf {
        self.0.join(next().to_string())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// THE FACTORY OF STORES OF RECORD: one new, empty store of each kind, at
/// `schema`. A third kind is one more line here.
pub(crate) fn stores_of_record<E: Schema>(
    scratch: &Scratch,
    schema: &E::Vocabulary,
) -> Vec<Box<dyn StoreOfRecord<E>>> {
    vec![Box::new(directory::<E>(scratch, schema))]
}

/// A new, empty store of files in `scratch`, at `schema`.
pub(crate) fn directory<E: Schema>(scratch: &Scratch, schema: &E::Vocabulary) -> EventStore<E> {
    EventStore::at(scratch.fresh(), Untrusted::none(), schema.clone())
}

/// One replica that the suite runs a law of replicas over. A store of
/// record has the operations of [`StoreOfRecord`] beside those of a
/// replica, and a store in memory has only those of a replica.
pub(crate) enum AnyReplica<E: Schema> {
    OfRecord(Box<dyn StoreOfRecord<E>>),
    Memory(Box<MemoryStore<E>>),
}

impl<E: Schema> AnyReplica<E> {
    pub(crate) fn replica(&self) -> &dyn Replica<E> {
        match self {
            AnyReplica::OfRecord(store) => store.replica(),
            AnyReplica::Memory(memory) => &**memory,
        }
    }
}

/// THE FACTORY OF REPLICAS: one new, empty replica of each kind, at
/// `schema`. Each store of record is one, and so is a store in memory.
pub(crate) fn replicas<E: Schema>(scratch: &Scratch, schema: &E::Vocabulary) -> Vec<AnyReplica<E>> {
    stores_of_record(scratch, schema)
        .into_iter()
        .map(AnyReplica::OfRecord)
        .chain([AnyReplica::Memory(Box::new(MemoryStore::at(
            schema.clone(),
        )))])
        .collect()
}

/// A store of record, as the suite drives it: the operations that only a
/// store of record has, and the doors to the place where it keeps its
/// prints.
///
/// A door changes what the store keeps without any handle of the store,
/// and takes no hold: `arrive` puts a print under a name that the store
/// does not keep, `overwrite` puts other bytes under a name that it keeps,
/// and `take_away` removes a print. Another program does each of these (a
/// copy of files, a `git merge`, a damaged disk, a host that deletes a
/// rejected proposal), and a write of the store that stops after a step has
/// done the steps before it. The suite states a stop after each step of a
/// write as the doors that the steps before it opened. The design note's
/// `Placing` names the same three writes `place`, `replace` and
/// `set_aside` when a holder makes them; a door is not a holder, so the
/// suite gives the doors other names. What `fsck` sets aside is read by
/// [`StoreOfRecord::aside`].
pub(crate) trait StoreOfRecord<E: Schema>: Replica<E> {
    /// This store as a replica.
    fn replica(&self) -> &dyn Replica<E>;

    /// A new, empty store of the same kind, at the same schema.
    fn sibling(&self) -> Box<dyn StoreOfRecord<E>>;

    /// A new handle on this store, as a new process opens it: it has read
    /// nothing.
    fn open(&self) -> Box<dyn StoreOfRecord<E>>;

    /// The store's exclusive hold, under which a decision reads and writes.
    ///
    /// # Errors
    ///
    /// The hold not taken, or a print that the store cannot read.
    fn hold(&self) -> Result<Box<dyn Hold<E> + '_>, ProdromeError>;

    /// A run of appends under one hold (law 76).
    ///
    /// # Errors
    ///
    /// The first event refused, with the names placed before it.
    fn append_all(&self, events: Vec<E>) -> Result<Vec<Hash>, Stopped>;

    /// The sealed parts of `names` dropped (SPEC §3). Answers the names
    /// husked.
    ///
    /// # Errors
    ///
    /// A print not replaced.
    fn husk(&self, names: BTreeSet<Hash>) -> Result<Vec<Hash>, ProdromeError>
    where
        E: Husking;

    /// Every print that fails its hash and is no husk set aside, then
    /// `verify`.
    fn fsck(&self) -> Vec<Finding>;

    /// Every finding of a cold read of the store.
    fn verify(&self) -> Vec<Finding>;

    /// The heads derived under the hold, and recorded.
    ///
    /// # Errors
    ///
    /// The hold not taken, or the record not written.
    fn record_heads(&self) -> Result<Heads, ProdromeError>;

    /// The events of the history, in its linear order.
    ///
    /// # Errors
    ///
    /// The store not read.
    fn events(&self) -> Result<Vec<E>, ProdromeError>;

    /// Every object that `name` rests on, and not `name`.
    ///
    /// # Errors
    ///
    /// `name`, or an object it rests on, not in the history.
    fn ancestors(&self, name: &Hash) -> Result<BTreeSet<Hash>, ProdromeError>;

    /// Every name that the store keeps, with the bytes under it, read past
    /// every handle: the history, what waits outside it, and what is no
    /// object. What a stopped write left, and a stray, are under no name,
    /// so they are not here; `verify` reports them.
    fn prints(&self) -> BTreeMap<Hash, Vec<u8>>;

    /// A door: `bytes` under `name`, which the store does not keep.
    fn arrive(&self, name: &Hash, bytes: &[u8]);

    /// A door: `bytes` under `name`, in place of the bytes kept there.
    fn overwrite(&self, name: &Hash, bytes: &[u8]);

    /// A door: the print under `name` taken away.
    fn take_away(&self, name: &Hash);

    /// What `fsck` has set aside, by the receipt that `verify` names, with
    /// the bytes.
    fn aside(&self) -> BTreeMap<String, Vec<u8>>;

    /// The receipt under which `fsck` sets aside `bytes`, kept under
    /// `name`, given what is set aside now.
    fn receipt(&self, name: &Hash, bytes: &[u8]) -> String;

    /// What a write leaves when it stops after it staged `bytes` and before
    /// the bytes took their name. Answers the finding of `verify` that
    /// names what it left.
    fn interrupt(&self, bytes: &[u8]) -> Finding;

    /// Does `finding` name only what a write that stopped left?
    fn left_by_a_stop(&self, finding: &Finding) -> bool;

    /// A fault: every placement under `name` fails until [`Self::clear`].
    fn obstruct(&self, name: &Hash);

    /// The fault of [`Self::obstruct`] removed.
    fn clear(&self, name: &Hash);

    /// The record of heads, read past every handle, where it is there and
    /// parses.
    fn record(&self) -> Option<Heads>;

    /// A door to the record of heads: removed, or these bytes in its place,
    /// as a power loss leaves a record cut short.
    fn set_record(&self, bytes: Option<&[u8]>);
}

/// The exclusive hold of a store of record: what one writer reads and
/// writes while no other writer finishes a write.
pub(crate) trait Hold<E: Schema> {
    /// Every object the store holds, as of the hold.
    fn dag(&self) -> Arc<Dag<E>>;

    /// One append under the hold, written through.
    ///
    /// # Errors
    ///
    /// The append's refusals.
    fn append(&mut self, event: E) -> Result<Hash, ProdromeError>;

    /// A run of appends under the hold.
    ///
    /// # Errors
    ///
    /// The first event refused, with the names placed before it.
    fn append_all(&mut self, events: Vec<E>) -> Result<Vec<Hash>, Stopped>;
}

impl<E: Schema> Hold<E> for Decision<'_, E, Untrusted> {
    fn dag(&self) -> Arc<Dag<E>> {
        Arc::clone(Decision::dag(self))
    }

    fn append(&mut self, event: E) -> Result<Hash, ProdromeError> {
        Decision::append(self, event)
    }

    fn append_all(&mut self, events: Vec<E>) -> Result<Vec<Hash>, Stopped> {
        Decision::append_all(self, events)
    }
}

/// The store of files: its prints are the files of `objects/`, named
/// `<name>.py`; `fsck` moves a print into `quarantine/`; the record of heads
/// is the file `.heads`; a stopped write leaves a temp file.
impl<E: Schema> StoreOfRecord<E> for EventStore<E> {
    fn replica(&self) -> &dyn Replica<E> {
        self
    }

    fn sibling(&self) -> Box<dyn StoreOfRecord<E>> {
        let root = self.root().with_file_name(format!("sibling-{}", next()));
        Box::new(EventStore::at(
            root,
            self.policy().clone(),
            self.schema().clone(),
        ))
    }

    fn open(&self) -> Box<dyn StoreOfRecord<E>> {
        Box::new(EventStore::at(
            self.root(),
            self.policy().clone(),
            self.schema().clone(),
        ))
    }

    fn hold(&self) -> Result<Box<dyn Hold<E> + '_>, ProdromeError> {
        Ok(Box::new(self.decide()?))
    }

    fn append_all(&self, events: Vec<E>) -> Result<Vec<Hash>, Stopped> {
        EventStore::append_all(self, events)
    }

    fn husk(&self, names: BTreeSet<Hash>) -> Result<Vec<Hash>, ProdromeError>
    where
        E: Husking,
    {
        EventStore::husk(self, names)
    }

    fn fsck(&self) -> Vec<Finding> {
        EventStore::fsck(self)
    }

    fn verify(&self) -> Vec<Finding> {
        EventStore::verify(self)
    }

    fn record_heads(&self) -> Result<Heads, ProdromeError> {
        EventStore::record_heads(self)
    }

    fn events(&self) -> Result<Vec<E>, ProdromeError> {
        EventStore::events(self)
    }

    fn ancestors(&self, name: &Hash) -> Result<BTreeSet<Hash>, ProdromeError> {
        EventStore::ancestors(self, name)
    }

    fn prints(&self) -> BTreeMap<Hash, Vec<u8>> {
        listed(&self.root().join("objects"))
            .into_iter()
            .filter_map(|(file, bytes)| {
                let name = Hash::new(file.strip_suffix(".py")?).ok()?;
                Some((name, bytes))
            })
            .collect()
    }

    fn arrive(&self, name: &Hash, bytes: &[u8]) {
        let objects = self.root().join("objects");
        fs::create_dir_all(&objects).expect("objects/ is made");
        fs::write(file_of(self.root(), name), bytes).expect("the print is placed");
    }

    fn overwrite(&self, name: &Hash, bytes: &[u8]) {
        fs::write(file_of(self.root(), name), bytes).expect("the print is replaced");
    }

    fn take_away(&self, name: &Hash) {
        fs::remove_file(file_of(self.root(), name)).expect("the print is taken away");
    }

    fn aside(&self) -> BTreeMap<String, Vec<u8>> {
        listed(&self.root().join("quarantine"))
    }

    /// `<name>.py`, or `<name>.<hash of the bytes>.py` where other bytes
    /// already lie under `<name>.py`, so that no bytes set aside are
    /// written over.
    fn receipt(&self, name: &Hash, bytes: &[u8]) -> String {
        let first = format!("{}.py", name.as_str());
        match self.aside().get(&first) {
            Some(held) if held.as_slice() != bytes => {
                let computed: String = Sha256::digest(bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                format!("{}.{computed}.py", name.as_str())
            }
            Some(_) | None => first,
        }
    }

    /// A temp file in `objects/`, named as the store names its temps:
    /// sixteen hex digits and `.tmp`.
    fn interrupt(&self, bytes: &[u8]) -> Finding {
        let objects = self.root().join("objects");
        fs::create_dir_all(&objects).expect("objects/ is made");
        let temp = format!("{:016x}.tmp", next());
        fs::write(objects.join(&temp), bytes).expect("the temp is written");
        Finding::Garbage(format!("objects/{temp}"))
    }

    fn left_by_a_stop(&self, finding: &Finding) -> bool {
        matches!(finding, Finding::Garbage(file) if file.ends_with(".tmp"))
    }

    /// A directory under the print's file name, onto which no rename lands.
    fn obstruct(&self, name: &Hash) {
        fs::create_dir_all(file_of(self.root(), name)).expect("the name is obstructed");
    }

    fn clear(&self, name: &Hash) {
        fs::remove_dir(file_of(self.root(), name)).expect("the name is cleared");
    }

    fn record(&self) -> Option<Heads> {
        let text = fs::read_to_string(self.root().join(".heads")).ok()?;
        Heads::parse(&text).ok()
    }

    fn set_record(&self, bytes: Option<&[u8]>) {
        let path = self.root().join(".heads");
        match bytes {
            Some(bytes) => fs::write(&path, bytes).expect("the record is written"),
            None => match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("the record is not removed: {error}"),
            },
        }
    }
}

/// The file of the object `name` in the store at `root`.
fn file_of(root: &Path, name: &Hash) -> PathBuf {
    root.join("objects").join(format!("{}.py", name.as_str()))
}

/// The files of `dir`, by name, with their bytes: none where `dir` is
/// absent.
fn listed(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let bytes = fs::read(entry.path()).expect("a listed file reads");
            (file, bytes)
        })
        .collect()
}

/// Every print `from` keeps, put in `to` by the door `arrive`.
pub(crate) fn copy_prints<E: Schema>(
    from: &(impl StoreOfRecord<E> + ?Sized),
    to: &(impl StoreOfRecord<E> + ?Sized),
) {
    for (name, bytes) in from.prints() {
        to.arrive(&name, &bytes);
    }
}

/// `replica` begun with a genesis whose nonce is fixed, so that every
/// replica of a case that is begun so is one prodrome. Answers the genesis.
pub(crate) fn begin<E: Schema>(replica: &(impl Replica<E> + ?Sized), label: &str) -> Hash {
    let genesis: Envelope<E> =
        Envelope::Genesis(mk_genesis(label, &"0".repeat(32)).expect("a genesis"));
    let name = seal_hash(&genesis);
    let print = canonical_envelope(&genesis).into_bytes();
    replica
        .receive(
            [name.clone()].into(),
            &|_| Some(print.clone()),
            Sender::Stranger,
        )
        .expect("begins");
    name
}

/// The heads that a fresh handle on `store` derives from its history.
pub(crate) fn derived<E: Schema>(store: &(impl StoreOfRecord<E> + ?Sized)) -> Heads {
    store
        .open()
        .held()
        .expect("a fresh handle reads")
        .dag
        .heads()
}
