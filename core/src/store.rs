//! §3 — the files around a [`Dag`]: objects on disk, the lock, durable
//! placement, quarantine, `adopt`/`merge`; and every other [`Replica`] of
//! the same objects (a [`MemoryStore`], an [`Overlay`]), [`sync`] between
//! any two, and the [`Decision`] only a store on disk can make.
//!
//! Objects live at `root/objects/<name>.py`, one canonical constructor
//! expression per file, the filename being the sha256 of its own bytes. THAT IS
//! THE WHOLE STORE. Its tips are DERIVED — the objects no object names as a
//! parent ([`Dag::tips`]) — so nothing on disk names a head, and two copies of a
//! store that each only added files are merged by putting their files in one
//! directory: a `git merge` of two clones IS the union, and nothing in it can
//! conflict. A store written before 0.9 also holds `HEAD` (and `refs/` while it
//! had several heads). Nothing reads or writes them any more — the objects
//! imply what they named (SPEC §9.17) — and `verify` asks for them to go.
//!
//! Effect shell, not pure core: the filesystem lives here and `literal`/`event`
//! stay pure. Two rules the reference states and this keeps:
//!
//! - a read VERIFIES, hashing the STORED BYTES against the filename before
//!   parsing — tamper-evidence on read — and never a reprint→rehash, which
//!   would couple every old object's validity to the current printer. A
//!   handle verifies each object once and holds it (`store::memory`); a held
//!   object is read again only when its file no longer looks as it did, and
//!   `verify` rehashes everything, on purpose;
//! - a write is content-addressed and idempotent, through a randomly named
//!   temp file synced before it is renamed, so the object appearing under its
//!   name IS the append: there is no second file to move after it, and no
//!   crash leaves a store half-written or an object empty.

mod decision;
mod memory;
mod memory_store;
mod overlay;
mod replica;

pub mod frontier;

use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::hash::{BuildHasher, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use crate::dag::{decode, Dag, Finding, Unread};
use crate::event::{mk_woven, parents_of, Envelope, Hash};
use crate::genesis::mk_genesis;
use crate::literal::ProdromeError;
use crate::policy::{Policy, Untrusted};
use crate::registers::{Folded, Node};
use crate::schema::Schema;
use crate::snapshot::mk_snapshot;

pub use decision::Decision;
use memory::{Memory, Printed, Seen, Verified};
pub use memory_store::MemoryStore;
pub use overlay::Overlay;
pub use replica::{accept, sync, Held, Replica};

/// A chain rooted at `root`, of the schema `E`, read under one host
/// [`Policy`] (§5).
///
/// THE STORE HOLDS THE POLICY, so callers do not each have to remember it and
/// `verify` has the one it is asked about. The type parameter defaults to
/// [`Untrusted`], the reference policy, because a store that names no policy
/// at all is a store nobody has configured — and `Untrusted::none()`, which
/// stands behind every writer, is the only honest thing for that to mean.
#[derive(Debug, Clone)]
pub struct EventStore<E: Schema, Pol = Untrusted> {
    root: PathBuf,
    policy: Pol,
    /// The schema the store is opened at: every print is parsed at it.
    schema: E::Vocabulary,
    /// WHAT THIS HANDLE (OR A CLONE OF IT) HAS READ: every object, verified
    /// once, and their fold. A read or an append looks at `objects/` and
    /// reads only what the memory does not hold, so it costs the objects
    /// new since the last look and not the store.
    ///
    /// The schema is the memory's: a store is parsed against one closed
    /// vocabulary (§2), the envelopes' and its schema's, so the schema is
    /// part of what a store IS, not an argument to each read. No object
    /// carries it.
    ///
    /// A `Mutex`, because every read may extend it, and a store is shared
    /// between threads: `EventStore` is `Send` and `Sync` wherever its
    /// schema's events and its policy are.
    memory: Arc<Mutex<Memory<E>>>,
    /// The prodrome this handle writes into, for a store that holds several.
    genesis: Option<Hash>,
}

/// `objects/`, read once: the entries named as objects, and every other
/// entry's file name.
#[derive(Debug, Default)]
struct Listing {
    objects: Vec<(Hash, fs::DirEntry)>,
    garbage: Vec<String>,
}

impl<E: Schema, Pol: Policy<E>> EventStore<E, Pol> {
    /// A store at a schema whose type says everything of it (a Rust
    /// schema's unit vocabulary).
    pub fn new(root: impl Into<PathBuf>, policy: Pol) -> EventStore<E, Pol>
    where
        E::Vocabulary: Default,
    {
        EventStore::at(root, policy, E::Vocabulary::default())
    }

    /// A store opened at the schema `schema`, a value: a declared schema
    /// admitted at run time ([`crate::declared`]), or a Rust schema's unit.
    pub fn at(root: impl Into<PathBuf>, policy: Pol, schema: E::Vocabulary) -> EventStore<E, Pol> {
        EventStore {
            root: root.into(),
            policy,
            schema,
            memory: Arc::new(Mutex::new(Memory::empty())),
            genesis: None,
        }
    }

    /// The memory, whoever held it last. A panic while it was held may have
    /// left it half extended, so a poisoned memory is forgotten and the next
    /// look reads the store as a fresh handle does.
    fn memory(&self) -> MutexGuard<'_, Memory<E>> {
        self.memory.lock().unwrap_or_else(|poisoned| {
            let mut memory = poisoned.into_inner();
            *memory = Memory::empty();
            self.memory.clear_poison();
            memory
        })
    }

    /// THE STORE OF RECORD'S DECISION: its lock taken, then its memory,
    /// brought level with `objects/`. See [`Decision`]. On unix only: there
    /// is no store, and no lock, off it.
    ///
    /// A READING MAY OMIT; A DECISION MAY NOT GUESS. Every read is total
    /// (SPEC law 45): a file that is no object is left out, with what rests
    /// on it, and named in [`Dag::excluded`]. A decision is not a reading.
    /// What it writes names the store's heads as its parents and deps, and
    /// what it answers ("is this still standing?") is a negation over the
    /// history: a file this reader cannot read may be a head, or the write
    /// that settled the question, and leaving it out would write a change
    /// over heads that are not the store's, or answer from a history the
    /// store does not hold. So a decision, and every write that begins with
    /// one, refuses while anything is unread, naming the file; `fsck` sets
    /// aside a file failing its hash, and a file that parses under a newer
    /// schema is read by opening the store at it.
    ///
    /// # Errors
    ///
    /// The lock not taken; `objects/` not listed; the first file, by name,
    /// that is not an object ([`Dag::whole`]).
    #[cfg(unix)]
    pub fn decide(&self) -> Result<Decision<'_, E, Pol>, ProdromeError> {
        self.decision()
    }

    /// [`EventStore::decide`], on every target: what each write begins with.
    fn decision(&self) -> Result<Decision<'_, E, Pol>, ProdromeError> {
        let locked = self.lock()?;
        let mut memory = self.memory();
        self.look(&mut memory)?;
        memory.dag().whole()?;
        Ok(Decision {
            store: self,
            memory,
            _locked: locked,
        })
    }

    /// Bring `memory` level with `objects/`. A name listed and not held is
    /// read and verified; a held name no longer listed is forgotten; a held
    /// file that no longer looks as it did when it was verified ([`Seen`]) is
    /// read and verified again. Nothing else is read, and nothing held is
    /// rehashed.
    ///
    /// TOTAL: what is read is read as a replica reads what it is handed
    /// ([`Dag::from_prints`]), so a file that is no object is held as unread
    /// with why, what rests on it waits, and every other object reads as it
    /// would had the file never been there (SPEC law 45). Refuses only where
    /// `objects/` cannot be listed.
    fn look(&self, memory: &mut Memory<E>) -> Result<(), ProdromeError> {
        let listed = self.listing()?.objects;
        let now = SystemTime::now();
        let gone: Vec<Hash> = {
            let listed: BTreeSet<&Hash> = listed.iter().map(|(name, _)| name).collect();
            memory
                .files()
                .keys()
                .filter(|name| !listed.contains(name))
                .cloned()
                .collect()
        };
        let mut seen = BTreeMap::new();
        for (name, entry) in listed {
            let at = Seen::of(stat(&entry), now);
            if !matches!(memory.files().get(&name), Some(Some(was)) if at.as_ref() == Some(was)) {
                seen.insert(name, at);
            }
        }
        let read = Dag::from_prints(
            &self.schema,
            seen.keys().map(|name| (name.clone(), self.raw(name))),
        );
        memory.level(gone, read, &seen);
        Ok(())
    }

    /// The policy this store reads under — what `verify` asks and what a
    /// caller folding its events should ask too, so that one store is one
    /// reading.
    pub fn policy(&self) -> &Pol {
        &self.policy
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn objects_dir(&self) -> PathBuf {
        self.root.join("objects")
    }

    fn object_path(&self, digest: &Hash) -> PathBuf {
        self.objects_dir().join(format!("{}.py", digest.as_str()))
    }

    /// Every head: the objects of the history no object of it names as a
    /// parent — [`Dag::tips`] over [`EventStore::dag`], and nothing on disk
    /// besides.
    ///
    /// Of the history the store reads, so a file that is no object is not
    /// among them, nor what rests on it: the heads of what verifies, and
    /// [`Dag::excluded`] says what they leave out. A write does not take
    /// them as the store's while anything is unread
    /// ([`EventStore::decide`]). Empty for an empty store. The tips are the
    /// memory's, so a steady store costs a directory listing.
    ///
    /// # Errors
    ///
    /// `objects/` not listed.
    pub fn tips(&self) -> Result<BTreeSet<Hash>, ProdromeError> {
        let mut memory = self.memory();
        self.look(&mut memory)?;
        Ok(memory.tips().clone())
    }

    /// What `objects/` holds, SPLIT BY NAME: an entry named `<name>.py` for a
    /// well-formed object name is an object, and every other entry is not one
    /// — a temp an interrupted write left, or a stray somebody put there.
    ///
    /// The reads take the objects and pass over the rest, so a crash that left
    /// a temp stops no read and no write; `verify` reports the rest, every
    /// entry of it, so nothing in the directory goes unsaid. In no particular
    /// order — nothing that reads it depends on one (SPEC §9.17). A store
    /// with no `objects/` yet holds nothing.
    fn listing(&self) -> Result<Listing, ProdromeError> {
        let dir = self.objects_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Listing::default())
            }
            Err(error) => return Err(io(&dir, &error)),
        };
        let mut listing = Listing::default();
        for entry in entries {
            let entry = entry.map_err(|e| io(&dir, &e))?;
            let file = entry.file_name().to_string_lossy().into_owned();
            match file.strip_suffix(".py").map(Hash::new) {
                Some(Ok(name)) => listing.objects.push((name, entry)),
                _ => listing.garbage.push(file),
            }
        }
        Ok(listing)
    }

    /// THE STORE'S HISTORY: the largest down-set of its objects
    /// ([`Dag::interior`]). An object resting on one the store lacks (set
    /// aside, not arrived, or a file that is no object) is in it once what
    /// it rests on is; until then no read sees it, and [`Dag::excluded`]
    /// names what it lacks and why, as it does for any replica holding the
    /// same files (SPEC law 45). What the memory holds, after reading what
    /// it does not.
    ///
    /// Shared, not copied: the memory goes on from this value, and copies it
    /// only if a look extends it while the caller still holds this one.
    ///
    /// # Errors
    ///
    /// `objects/` not listed.
    pub fn dag(&self) -> Result<Arc<Dag<E>>, ProdromeError> {
        let mut memory = self.memory();
        self.look(&mut memory)?;
        Ok(Arc::clone(memory.dag()))
    }

    /// The fold of the store's history (what an append decides on):
    /// `fold(&dag.nodes()?)` of [`EventStore::dag`], extended by each object
    /// as it is read and never refolded from the files. Shared as `dag` is: a caller holding it
    /// while the store extends its memory makes that extension copy it.
    pub fn folded(&self) -> Result<Arc<Folded<E>>, ProdromeError> {
        let mut memory = self.memory();
        self.look(&mut memory)?;
        memory.folded().map(Arc::clone)
    }

    fn lock_file(&self) -> PathBuf {
        self.root.join(".lock")
    }

    /// AN EXCLUSIVE `flock` ON `root/.lock`, HELD FOR ONE WRITE.
    ///
    /// The same file and the same operation the earlier Python writer took
    /// (the reference's `store.py::_flock`), and that is the whole point: a
    /// store may be appended to by more than one program, and a second writer
    /// is not always this crate. Two writers that lock different files are two
    /// writers that do not lock — an append is read-the-heads,
    /// write-the-object, and two of those interleaved fork the chain — which
    /// derived tips make harmless (the next write weaves both) but not
    /// intended.
    ///
    /// `flock` is per open file description, so this is advisory BETWEEN
    /// PROCESSES and says nothing between the threads of one. An application
    /// that decides something FROM the chain and then appends (the server's
    /// existence checks, its id search, its stamp) serialises its own threads
    /// above this; the lock here covers the append itself.
    ///
    /// The guard is the `File`: the lock releases when the last descriptor for
    /// the description closes, so dropping it unlocks on every path out,
    /// including an early `?`.
    #[cfg(unix)]
    fn lock(&self) -> Result<Option<File>, ProdromeError> {
        use rustix::fs::{flock, FlockOperation};
        make_dir(&self.root)?;
        let path = self.lock_file();
        let handle = File::create(&path).map_err(|e| io(&path, &e))?;
        flock(&handle, FlockOperation::LockExclusive)
            .map_err(|e| io(&path, &std::io::Error::from(e)))?;
        Ok(Some(handle))
    }

    /// No lock off unix, because there is no store off unix: `prodrome-wasm`
    /// compiles this crate for the browser, where `root` is a path nothing
    /// reads. Stated rather than `#[cfg]`-ed away at the call sites, so the
    /// write paths below read the same on every target.
    #[cfg(not(unix))]
    fn lock(&self) -> Result<Option<File>, ProdromeError> {
        Ok(None)
    }

    /// This handle, writing into the prodrome `genesis` begins; without one
    /// a handle writes into the store's only genesis.
    pub fn in_genesis(self, genesis: Hash) -> EventStore<E, Pol> {
        EventStore {
            genesis: Some(genesis),
            ..self
        }
    }

    /// Begin a prodrome: write a `Genesis(label, nonce)` into a store that
    /// has none, and answer its name.
    pub fn init(&self, label: &str) -> Result<Hash, ProdromeError> {
        let mut decision = self.decision()?;
        if let Some(genesis) = decision.memory.geneses().first() {
            return Err(ProdromeError::Store(format!(
                "the store already has a genesis, {}",
                genesis.as_str()
            )));
        }
        let nonce = format!("{:016x}{:016x}", random(), random());
        let genesis = Envelope::Genesis(mk_genesis(label, &nonce)?);
        self.write(&mut decision.memory, Verified::sealed(genesis))
    }

    /// Write `event` as a `Change` over its entity's heads (every write to
    /// that entity the writer holds that no other descends from), or, when
    /// the writer's prodrome already holds an object whose event prints the
    /// same, write nothing and answer the first such. A write that would take
    /// an inflationary register below the reading it supersedes is refused,
    /// and nothing is written.
    ///
    /// DECIDED ON THE MEMORY, UNDER THE LOCK. The look that brings the memory
    /// level with `objects/` is taken after the lock is, and every writer
    /// places its object before it lets the lock go, so the memory holds
    /// every object any writer finished: the twin check and the deps read
    /// what a fresh handle would read, and cost what is new.
    pub fn append(&self, event: E) -> Result<Hash, ProdromeError> {
        self.decision()?.append(event)
    }

    /// Attest the writer's prodrome: a `Snapshot` of its tips, chained to the
    /// last snapshot among them. With nothing new since that one, it is the
    /// answer and nothing is written.
    pub fn snapshot(&self) -> Result<Hash, ProdromeError> {
        let mut decision = self.decision()?;
        let snapshot = {
            let genesis = decision.memory.writer(self.genesis.as_ref())?;
            let dag = decision.memory.dag();
            let tips = dag.tips_in(&dag.key(&genesis));
            let previous = dag.linearise()?.into_iter().rev().find(|name| {
                tips.contains(name) && matches!(dag.get(name), Some(Envelope::Snapshot(_)))
            });
            let tips: Vec<Hash> = tips
                .into_iter()
                .filter(|tip| Some(tip) != previous.as_ref())
                .collect();
            match previous {
                Some(previous) if tips.is_empty() => return Ok(previous),
                previous => mk_snapshot(genesis, tips, previous)?,
            }
        };
        self.write(
            &mut decision.memory,
            Verified::sealed(Envelope::Snapshot(snapshot)),
        )
    }

    /// Join two or more tips into one `Woven`, which becomes a tip in their
    /// place: a legacy store's merge, which a store of changes never needs.
    ///
    /// `parents` of `None` means every tip of the legacy prodrome. NO
    /// who/when/why: a merge asserts structure, not a fact about a todo. A
    /// caller that wants the act attributed passes an ordinary `event`, and
    /// the object is a write and a join at once.
    pub fn merge(&self, parents: Option<&[Hash]>, event: Option<E>) -> Result<Hash, ProdromeError> {
        let mut decision = self.decision()?;
        let on: Vec<Hash> = match parents {
            Some(named) => named.to_vec(),
            None => decision.memory.dag().tips_in(&None).into_iter().collect(),
        };
        self.require_present(&on)?;
        self.write(&mut decision.memory, Verified::sealed(mk_woven(on, event)?))
    }

    /// Take in everything another replica's `digest` rests on, verified.
    ///
    /// THE ONLY WAY A SECOND TIP APPEARS, and there is no placement to do: the
    /// tips are derived, so git's three cases fall out of the objects. A tip we
    /// already contain changes nothing; a tip that contains every tip we hold
    /// names them all beneath it, so they stop being tips (a FAST-FORWARD);
    /// anything else is a second tip. Answers `digest`.
    ///
    /// [`accept`], the restricted join, with one tip chosen.
    pub fn adopt(&self, source: &EventStore<E, Pol>, digest: &Hash) -> Result<Hash, ProdromeError> {
        accept(source, self, [digest.clone()].into())?;
        Ok(digest.clone())
    }

    /// The same adoption from a replica that is NOT a directory: `objects` maps
    /// a name to the canonical print that name is the hash of.
    ///
    /// A replica that reaches this store over a wire arrives as BYTES, and the
    /// alternative was for its caller to lay them out as a second store —
    /// a temporary directory whose layout is this module's own business,
    /// written by somebody who is not this module. The placement rule, the
    /// reverification and the walk over parents are IDENTICAL; only where the
    /// bytes are read from differs, which is why [`EventStore::adopt`] is now
    /// this function with a directory for a source.
    ///
    /// The same refusals, in the same words: bytes that do not hash to the name
    /// given, an object that does not parse, and a parent neither `objects` nor
    /// this store holds.
    pub fn adopt_objects(
        &self,
        objects: &BTreeMap<Hash, String>,
        digest: &Hash,
    ) -> Result<Hash, ProdromeError> {
        self.copy_in_from([digest.clone()], |name| {
            objects.get(name).map(|text| text.as_bytes().to_vec())
        })?;
        Ok(digest.clone())
    }

    /// A parent must be an object we hold. Refused here rather than left to
    /// `verify`, because an object naming a parent nobody has is a broken store
    /// written deliberately, and the store is what knows.
    fn require_present(&self, parents: &[Hash]) -> Result<(), ProdromeError> {
        for parent in parents {
            if !self.object_path(parent).exists() {
                return Err(ProdromeError::Store(format!(
                    "missing object {}, named as a parent",
                    parent.as_str()
                )));
            }
        }
        Ok(())
    }

    /// The adoption every door shares, under the lock: what `seeds` rest
    /// on that this store lacks, [`memory::receive`]d (each object read by
    /// `print_of` and VERIFIED on the way in, a replica not trusted for
    /// being a replica), then placed. Answers what it placed, parents first.
    ///
    /// The walk ends at what the memory holds: this store is closed under
    /// parents, so everything above one of ours is already here. A store
    /// holding a file that does not verify refuses, as every write does
    /// ([`EventStore::decide`]), naming the file and the quarantine that
    /// sets it aside; adopting the object back is the repair after that.
    ///
    /// NOTHING IS WRITTEN UNTIL EVERYTHING HAS VERIFIED, and then PARENTS
    /// FIRST. With derived tips an object is part of the store the moment its
    /// file appears, so a child written before its parent — by a crash between
    /// the two, or read by another process in between — would be a store
    /// naming an object it does not hold. Written in this order, every prefix
    /// of the adoption is a store closed under parents, and an adoption cut
    /// short is completed by the next. What is written was verified on the
    /// way in, so it enters the memory without a second read.
    fn copy_in_from(
        &self,
        seeds: impl IntoIterator<Item = Hash>,
        print_of: impl Fn(&Hash) -> Option<Vec<u8>>,
    ) -> Result<Vec<Hash>, ProdromeError> {
        let mut decision = self.decision()?;
        let memory = &mut decision.memory;
        let taken = memory::receive(&self.schema, |name| memory.holds(name), seeds, print_of)?;
        // A waiting object is walked through, to what it waits for, and its
        // file, already here, is not written again.
        self.place(
            taken
                .iter()
                .filter(|(verified, _)| !memory.files().contains_key(verified.name()))
                .map(|(verified, print)| (verified.name(), print.as_slice())),
        )?;
        let now = SystemTime::now();
        let names = taken
            .iter()
            .map(|(verified, _)| verified.name().clone())
            .collect();
        let seen = taken
            .into_iter()
            .map(|(verified, _)| {
                let seen = Seen::of(fs::metadata(self.object_path(verified.name())).ok(), now);
                (verified, seen)
            })
            .collect();
        memory.admit(&[], seen);
        Ok(names)
    }

    /// Objects onto disk under their names, in the order given, DURABLY:
    /// each is [`put_durably`] without the directory sync, and `objects/` is
    /// synced once when all of them are, so a batch — one adoption, one
    /// append — costs one directory sync and not one per object. The store's
    /// `.gitattributes` goes first, the same way, where there is none.
    fn place<'a>(
        &self,
        objects: impl IntoIterator<Item = (&'a Hash, &'a [u8])>,
    ) -> Result<(), ProdromeError> {
        let dir = self.objects_dir();
        make_dir(&dir)?;
        self.gitattributes()?;
        for (digest, bytes) in objects {
            rename_into(&stage(&dir, bytes)?, &self.object_path(digest))?;
        }
        sync_dir(&dir)
    }

    /// `root/.gitattributes`, written once, durably, where there is none:
    /// [`GITATTRIBUTES`], so that git never rewrites a byte a name is the
    /// hash of, wherever in a repository the store lies. One a host already
    /// has is the host's, and is never rewritten.
    fn gitattributes(&self) -> Result<(), ProdromeError> {
        let path = self.root.join(".gitattributes");
        if path.exists() {
            return Ok(());
        }
        put_durably(&path, GITATTRIBUTES.as_bytes())
    }

    /// One object onto disk, content-addressed and idempotent, and into
    /// `memory`, which has just looked: an object it holds is a no-op while
    /// its file is there, since the look found it holding its bytes;
    /// DIFFERENT bytes already at
    /// the name are a sha256 collision or a corrupt store, and we stop rather
    /// than clobber. The object is verified because it was sealed here, so
    /// nothing written is read back.
    fn write(
        &self,
        memory: &mut Memory<E>,
        (verified, text): Printed<E>,
    ) -> Result<Hash, ProdromeError> {
        let digest = verified.name().clone();
        let path = self.object_path(&digest);
        if memory.holds(&digest) && path.exists() {
            return Ok(digest);
        }
        if path.exists() {
            let existing = fs::read(&path).map_err(|e| io(&path, &e))?;
            if existing != text {
                return Err(ProdromeError::Store(format!(
                    "object {} exists with different bytes — sha256 collision or corruption",
                    digest.as_str()
                )));
            }
        } else {
            self.place([(&digest, text.as_slice())])?;
        }
        let seen = Seen::of(fs::metadata(&path).ok(), SystemTime::now());
        memory.admit(&[], vec![(verified, seen)]);
        Ok(digest)
    }

    /// Load and REVERIFY one object: hash the STORED BYTES against the
    /// filename, then parse. Deliberately NOT a reparse→reprint→rehash — git
    /// hashes bytes and not semantics, so that a future printer change cannot
    /// false-alarm the whole store as tampered.
    pub fn load(&self, digest: &Hash) -> Result<Envelope<E>, ProdromeError> {
        decode(&self.schema, digest, &self.raw(digest)?).map_err(|why| why.refusal(digest))
    }

    /// The print held under `name`, unchecked: what an adoption from this
    /// store reads, and verifies.
    fn print(&self, name: &Hash) -> Option<Vec<u8>> {
        fs::read(self.object_path(name)).ok()
    }

    /// One object file's bytes, unchecked.
    fn raw(&self, digest: &Hash) -> Result<Vec<u8>, ProdromeError> {
        let path = self.object_path(digest);
        if !path.exists() {
            return Err(ProdromeError::Store(format!(
                "missing object {}",
                digest.as_str()
            )));
        }
        fs::read(&path).map_err(|e| io(&path, &e))
    }

    fn quarantine_dir(&self) -> PathBuf {
        self.root.join("quarantine")
    }

    /// Where a file under `digest` whose bytes hash to `computed` is set
    /// aside: `<name>.py`, or, where other bytes already lie there,
    /// `<name>.<computed>.py`, named by its own bytes as everything in the
    /// store is. So no set-aside bytes are ever overwritten by others, and
    /// the same bytes set aside twice are one file.
    fn aside_path(&self, digest: &Hash, computed: &Hash) -> PathBuf {
        let first = self
            .quarantine_dir()
            .join(format!("{}.py", digest.as_str()));
        match fs::read(&first) {
            Ok(held) if Hash::of_bytes(&held) != *computed => {
                self.quarantine_dir()
                    .join(format!("{}.{}.py", digest.as_str(), computed.as_str()))
            }
            _ => first,
        }
    }

    /// FULL FSCK: set aside every object file that fails its hash, then
    /// [`EventStore::verify`]. An empty result means healthy.
    ///
    /// A file whose bytes are not the hash of its name has no honest reading:
    /// every read leaves it out, and every write refuses to guess what it
    /// held, so one damaged file would stop every write. This moves each
    /// such file to `quarantine/` (durably, under the store's lock, never
    /// overwriting other bytes already there and never deleting any), and
    /// the store then reads and writes the largest down-set it holds
    /// (law 40): the history WITHOUT the object and without
    /// everything resting on it, which a replica that never received the
    /// object holds. The report names each set-aside file
    /// ([`Finding::Quarantined`]) until the object is restored from a
    /// replica (an `adopt`, or a copy of its file) and the receipt deleted;
    /// what rested on the object rejoins the history with it.
    ///
    /// ONLY A FILE THAT FAILS ITS HASH. One that hashes to its name is that
    /// object, whatever it holds — one this reader cannot parse may be a
    /// newer writer's — and setting it aside would be hiding it: it is
    /// reported, and stays. Strays are reported, never moved or deleted.
    ///
    /// So such a file blocks writes until its owner decides what it is,
    /// which nothing here can: a newer writer's object is read, and written
    /// over, by opening the store at the schema that parses it; a print no
    /// schema parses (it is not text, or it is junk under an object's name)
    /// is no object of any reader's, and its owner moves it out of
    /// `objects/` by hand once `verify` has named it. Reads answer
    /// throughout, leaving it out.
    #[must_use]
    pub fn fsck(&self) -> Vec<Finding> {
        match self.quarantine() {
            Ok(()) => self.verify(),
            Err(error) => std::iter::once(Finding::Io(error))
                .chain(self.verify())
                .collect(),
        }
    }

    /// Every file of `objects/` failing its hash, moved to `quarantine/`
    /// under the lock: the directory it lands in synced with each, and
    /// `objects/` once after. The files are those a cold look holds as
    /// unread ([`EventStore::cold`]), every one rehashed.
    fn quarantine(&self) -> Result<(), ProdromeError> {
        let _locked = self.lock()?;
        let read = self.cold()?;
        let tampered: Vec<(&Hash, &Hash)> = read
            .dag()
            .unread()
            .iter()
            .filter_map(|(name, why)| match why {
                Unread::Tampered(computed) => Some((name, computed)),
                _ => None,
            })
            .collect();
        if tampered.is_empty() {
            return Ok(());
        }
        make_dir(&self.quarantine_dir())?;
        for (name, computed) in tampered {
            rename_into(&self.object_path(name), &self.aside_path(name, computed))?;
        }
        sync_dir(&self.objects_dir())
    }

    /// Everything `digest` transitively rests on — STRICTLY: an object is not
    /// its own ancestor. This is the partial order and the whole of it.
    pub fn ancestors(&self, digest: &Hash) -> Result<BTreeSet<Hash>, ProdromeError> {
        let dag = self.dag()?;
        let parents = |name: &Hash| {
            dag.get(name)
                .map(parents_of)
                .ok_or_else(|| ProdromeError::Store(format!("missing object {}", name.as_str())))
        };
        let mut found: BTreeSet<Hash> = BTreeSet::new();
        let mut pending = parents(digest)?;
        while let Some(name) = pending.pop() {
            if !found.insert(name.clone()) {
                continue;
            }
            pending.extend(parents(&name)?);
        }
        Ok(found)
    }

    /// Do these two objects know nothing of each other? Neither rests on the
    /// other — the definition, and the only one this store has: not "written at
    /// the same time", which is a claim about clocks, and not "on different
    /// branches", which is a claim about names.
    pub fn concurrent(&self, a: &Hash, b: &Hash) -> Result<bool, ProdromeError> {
        if a == b {
            return Ok(false);
        }
        Ok(!self.ancestors(a)?.contains(b) && !self.ancestors(b)?.contains(a))
    }

    /// The store's events, in the linearisation's order.
    pub fn events(&self) -> Result<Vec<E>, ProdromeError> {
        Ok(self
            .dag()?
            .nodes()?
            .into_iter()
            .filter_map(Node::into_event)
            .collect())
    }

    /// What a fresh handle reads: every file read, and so rehashed, on
    /// purpose, whatever this handle's memory holds. The look every read
    /// takes, into an empty memory.
    fn cold(&self) -> Result<Memory<E>, ProdromeError> {
        let mut memory = Memory::empty();
        self.look(&mut memory)?;
        Ok(memory)
    }

    /// Full fsck; an empty result means healthy.
    ///
    /// The [`Dag`]'s findings of a cold look ([`EventStore::cold`]), the
    /// unread prints and what rests on them as [`Dag::excluded`] says, with
    /// what only the files can say beside them:
    /// every entry of `objects/` that is not an object, a `HEAD` or `refs/`
    /// left from before the tips were derived, and a receipt for each file in
    /// `quarantine/`, which stands in for the missing parent it would be.
    /// Asked under the store's policy at what it read ([`Policy::at`]).
    pub fn verify(&self) -> Vec<Finding> {
        let (garbage, read) = match self
            .listing()
            .and_then(|listing| Ok((listing, self.cold()?)))
        {
            Ok((Listing { garbage, .. }, read)) => (garbage, read),
            Err(error) => return vec![Finding::Io(error)],
        };
        let mut garbage: Vec<String> = garbage
            .into_iter()
            .map(|file| format!("objects/{file}"))
            .collect();
        garbage.sort();
        let dag = read.dag();
        let (mut findings, graph): (Vec<Finding>, Vec<Finding>) = dag
            .verify(self.policy.at(dag).as_ref().unwrap_or(&self.policy))
            .into_iter()
            .partition(|finding| matches!(finding, Finding::Unread { .. }));
        garbage.extend(self.temps());
        findings.extend(garbage.into_iter().map(Finding::Garbage));
        findings.extend(
            ["HEAD", "refs/"]
                .into_iter()
                .filter(|file| self.root.join(file.trim_end_matches('/')).exists())
                .map(Finding::Leftover),
        );
        let aside = match self.aside() {
            Ok(aside) => aside,
            Err(error) => {
                findings.push(Finding::Io(error));
                Vec::new()
            }
        };
        let receipts: BTreeSet<&str> = aside
            .iter()
            .filter_map(|file| file.split('.').next())
            .collect();
        findings.extend(aside.iter().cloned().map(Finding::Quarantined));
        findings.extend(graph.into_iter().filter(|finding| {
            !matches!(finding, Finding::Broken { at, .. } if receipts.contains(at.as_str()))
        }));
        findings
    }

    /// The temps an interrupted write left in the root, where the store's
    /// own `.gitattributes` is written: each by name, sorted. Any other file
    /// in the root is the host's.
    fn temps(&self) -> Vec<String> {
        let mut temps: Vec<String> = fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|file| is_temp(file))
            .collect();
        temps.sort();
        temps
    }

    /// The files in `quarantine/`, sorted: none where it does not exist.
    fn aside(&self) -> Result<Vec<String>, ProdromeError> {
        let dir = self.quarantine_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(io(&dir, &error)),
        };
        let mut files: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        Ok(files)
    }
}

/// The store on disk as a replica: what it holds is `objects/`, and what it
/// receives is placed there, durably, under its lock.
impl<E: Schema, Pol: Policy<E>> Replica<E> for EventStore<E, Pol> {
    fn schema(&self) -> &E::Vocabulary {
        &self.schema
    }

    /// Total, as every read of the store is: what it cannot read is left
    /// out, and its `dag` says so.
    fn held(&self) -> Result<Held<E>, ProdromeError> {
        let mut memory = self.memory();
        self.look(&mut memory)?;
        let folded = memory.folded()?.clone();
        Ok(Held {
            dag: memory.dag().clone(),
            folded,
            tips: memory.tips().clone(),
        })
    }

    fn tips(&self) -> Result<BTreeSet<Hash>, ProdromeError> {
        EventStore::tips(self)
    }

    fn print(&self, name: &Hash) -> Option<Vec<u8>> {
        EventStore::print(self, name)
    }

    fn append(&self, event: E) -> Result<Hash, ProdromeError> {
        EventStore::append(self, event)
    }

    fn receive(
        &self,
        seeds: BTreeSet<Hash>,
        print_of: &dyn Fn(&Hash) -> Option<Vec<u8>>,
    ) -> Result<Vec<Hash>, ProdromeError> {
        self.copy_in_from(seeds, print_of)
    }
}

/// A listed file's metadata, through a symlink to what it names, since
/// that is what a read of it reads.
fn stat(entry: &fs::DirEntry) -> Option<fs::Metadata> {
    match entry.file_type() {
        Ok(kind) if !kind.is_symlink() => entry.metadata().ok(),
        _ => fs::metadata(entry.path()).ok(),
    }
}

/// What a store writes as its own `.gitattributes`: its objects, and what
/// it set aside, are bytes named by their hash, never text to convert,
/// diff or merge (SPEC §3).
const GITATTRIBUTES: &str = "\
# A prodrome store (SPEC §3): each file is named by the sha256 of its bytes,
# so git must never rewrite one: no line-ending conversion, no text merge.
objects/** -text -diff
quarantine/** -text -diff
";

/// THE ONE DURABLE WRITE: `bytes` at `path`, through a temp file in the same
/// directory ([`stage`]), synced, renamed over `path`, and the directory
/// synced. Every file the store writes is written so, or is a batch of
/// these sharing the last sync ([`EventStore::place`]).
///
/// The file is synced BEFORE its rename, which is the order that matters: a
/// rename can reach the disk before the bytes it names, and a power loss
/// between the two would leave a zero-length file under a name that
/// promises content. Synced first, a crash anywhere leaves either nothing at
/// `path` or the whole file, and at worst a temp that `verify` reports.
fn put_durably(path: &Path, bytes: &[u8]) -> Result<(), ProdromeError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    make_dir(dir)?;
    rename_into(&stage(dir, bytes)?, path)?;
    sync_dir(dir)
}

/// Bytes into a fresh temp file in `dir`, synced to disk: the first half of
/// a durable write, and all a crash before the rename leaves behind.
///
/// The name is RANDOM and the file is created EXCLUSIVELY, never a fixed
/// `<name>.tmp`: two writers placing the same object would otherwise write
/// into one temp together, and one of them rename the other's half. A temp
/// is not named like an object, so no read takes it for one.
fn stage(dir: &Path, bytes: &[u8]) -> Result<PathBuf, ProdromeError> {
    loop {
        let temp = dir.join(temp_name());
        match File::options().write(true).create_new(true).open(&temp) {
            Ok(mut file) => {
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|e| io(&temp, &e))?;
                return Ok(temp);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io(&temp, &error)),
        }
    }
}

/// `from` renamed to `to`, and `to`'s directory synced when it is not
/// `from`'s, whose sync the caller owes (one per batch).
fn rename_into(from: &Path, to: &Path) -> Result<(), ProdromeError> {
    fs::rename(from, to).map_err(|e| io(from, &e))?;
    match to.parent() {
        Some(dir) if Some(dir) != from.parent() => sync_dir(dir),
        _ => Ok(()),
    }
}

/// `dir`, made if it is missing, DURABLY: each directory made is synced
/// into its parent, so a crash after a write into it cannot lose the
/// directory, and so the write, while the file's own sync held.
fn make_dir(dir: &Path) -> Result<(), ProdromeError> {
    if dir.is_dir() {
        return Ok(());
    }
    let mut made = Vec::new();
    let mut missing = Some(dir);
    while let Some(at) = missing.filter(|at| !at.as_os_str().is_empty() && !at.exists()) {
        made.push(at);
        missing = at.parent();
    }
    fs::create_dir_all(dir).map_err(|e| io(dir, &e))?;
    for at in made.into_iter().rev() {
        if let Some(parent) = at.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            sync_dir(parent)?;
        }
    }
    Ok(())
}

/// A temp file's name: sixteen random hex digits and `.tmp`, which is never
/// `<name>.py`. The randomness is the standard library's per-process hash
/// keys, advanced on every draw, so no dependency is taken for it; the
/// exclusive create in [`stage`] is what makes a clash harmless.
fn temp_name() -> String {
    format!("{:016x}.tmp", random())
}

/// Is `file` a name [`temp_name`] draws?
fn is_temp(file: &str) -> bool {
    file.strip_suffix(".tmp")
        .is_some_and(|stem| stem.len() == 16 && stem.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn random() -> u64 {
    RandomState::new().build_hasher().finish()
}

/// Sync a directory, so the renames into it are on disk and not only in the
/// page cache.
fn sync_dir(dir: &Path) -> Result<(), ProdromeError> {
    File::open(dir)
        .and_then(|handle| handle.sync_all())
        .map_err(|error| io(dir, &error))
}

fn io(path: &Path, error: &std::io::Error) -> ProdromeError {
    ProdromeError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::Change;
    use crate::event::{
        mk_completed, mk_created, mk_reopened, mk_sealed, seal_hash, Actor, TodoEvent,
    };
    use crate::literal::Datetime;
    use crate::reference::Todo;
    use proptest::prelude::*;

    /// The store these tests drive. The schema is the todo one, with the
    /// reference payload, because a store has to hold SOME schema; nothing
    /// below reads a field of a record.
    type Store = EventStore<TodoEvent<Todo>>;

    fn at(day: u32) -> Datetime {
        Datetime::new(2026, 9, day, 12, 0, 0, 0).expect("a real instant")
    }

    /// The reference policy, with one name on the roster — every store below
    /// reads under it, and only the dating test can tell.
    fn roster() -> Untrusted {
        Untrusted::of([Actor::new("triage").expect("valid")])
    }

    fn tips(store: &Store) -> BTreeSet<Hash> {
        store.tips().expect("the tips derive")
    }

    fn findings(store: &Store) -> Vec<String> {
        store.verify().iter().map(ToString::to_string).collect()
    }

    fn order(store: &Store) -> Result<Vec<Hash>, ProdromeError> {
        store.dag()?.linearise()
    }

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "prodrome-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        root
    }

    /// The pre-Draft-A append, for the tests of a legacy store: seal on
    /// every tip.
    fn seal(store: &Store, event: TodoEvent<Todo>) -> Hash {
        let on: Vec<Hash> = tips(store).into_iter().collect();
        let object = if on.len() > 1 {
            mk_woven(on, Some(event)).expect("distinct tips")
        } else {
            mk_sealed(on.into_iter().next(), event)
        };
        put(store, object).expect("writes")
    }

    /// One object written as the writers above write theirs: into a memory
    /// that has just looked.
    fn put(store: &Store, object: Envelope<TodoEvent<Todo>>) -> Result<Hash, ProdromeError> {
        let mut memory = store.memory();
        store.look(&mut memory)?;
        store.write(&mut memory, Verified::sealed(object))
    }

    fn change(store: &Store, name: &Hash) -> Change<TodoEvent<Todo>> {
        match store.load(name).expect("loads") {
            Envelope::Change(change) => change,
            other => panic!("{} is not a change", other.name()),
        }
    }

    /// An append writes a `Change` over its entity's heads, whatever
    /// registers they write, and appending the same event again writes
    /// nothing.
    #[test]
    fn an_append_writes_a_change_over_what_it_supersedes() {
        let store = Store::new(scratch("chain"), roster());
        let created = mk_created("alpha", at(1), "bassel", "", "").expect("valid");
        assert!(store.append(created.clone()).is_err(), "no genesis yet");
        let genesis = store.init("chain").expect("begins");
        assert!(store.init("again").is_err(), "a store begins once");
        let first = store.append(created).expect("appends");
        let done = mk_completed("alpha", at(2), "bassel", "").expect("valid");
        let second = store.append(done.clone()).expect("appends");
        let third = store
            .append(mk_reopened("alpha", at(3), "bassel", "").expect("valid"))
            .expect("appends");
        assert_eq!(change(&store, &first).genesis, genesis);
        assert_eq!(
            change(&store, &second).deps,
            std::slice::from_ref(&first),
            "Created writes no register, and the writer saw it"
        );
        assert_eq!(change(&store, &third).deps, std::slice::from_ref(&second));
        assert_eq!(
            tips(&store),
            [third.clone()].into_iter().collect(),
            "the genesis is beneath its changes"
        );
        assert_eq!(
            fs::read_dir(store.root())
                .expect("the store is a directory")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name != ".lock")
                .collect::<BTreeSet<_>>(),
            [".gitattributes".to_owned(), "objects".to_owned()]
                .into_iter()
                .collect(),
            "the objects are the whole store: no file names a head"
        );
        assert_eq!(
            fs::read_to_string(store.root().join(".gitattributes")).expect("written"),
            GITATTRIBUTES,
            "the store says, wherever it lies in a repository, that git may not rewrite it"
        );
        assert_eq!(findings(&store), Vec::<String>::new());
        assert_eq!(store.events().expect("reads").len(), 3);
        assert_eq!(
            store.ancestors(&third).expect("walks"),
            [genesis, first.clone(), second.clone()]
                .into_iter()
                .collect(),
            "a change rests on its genesis"
        );
        let held = order(&store).expect("reads");
        assert_eq!(store.append(done).expect("replays"), second);
        assert_eq!(
            order(&store).expect("reads"),
            held,
            "a replay writes nothing"
        );
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn adopt_makes_a_second_head_and_merge_settles_it() {
        let here = Store::new(scratch("here"), roster());
        let there = Store::new(scratch("there"), roster());
        let shared = seal(
            &here,
            mk_created("alpha", at(1), "bassel", "", "").expect("valid"),
        );
        there
            .adopt(&here, &shared)
            .expect("fast-forwards into an empty store");
        assert_eq!(tips(&there), [shared.clone()].into_iter().collect());

        let mine = seal(
            &here,
            mk_created("beta", at(2), "bassel", "mine", "").expect("valid"),
        );
        let theirs = seal(
            &there,
            mk_created("beta", at(2), "bassel", "theirs", "").expect("valid"),
        );
        here.adopt(&there, &theirs).expect("adopts");
        assert_eq!(
            tips(&here),
            [mine.clone(), theirs.clone()].into_iter().collect()
        );
        assert!(here.concurrent(&mine, &theirs).expect("both present"));
        assert_eq!(findings(&here), Vec::<String>::new());

        let merged = here.merge(None, None).expect("merges");
        assert_eq!(tips(&here), [merged.clone()].into_iter().collect());
        assert_eq!(findings(&here), Vec::<String>::new());
        // Sorted by `mk_woven`, so a merge is its parent SET and two replicas
        // joining the same two heads produce the same bytes.
        let mut joined = vec![mine, theirs];
        joined.sort();
        assert_eq!(parents_of(&here.load(&merged).expect("loads")), joined);
        // A merge carries no event, so it contributes none.
        assert_eq!(here.events().expect("reads").len(), 3);
        let _ = fs::remove_dir_all(here.root());
        let _ = fs::remove_dir_all(there.root());
    }

    /// THE NEXT WRITE SETTLES A UNION. Two replicas' files put in one
    /// directory, which is what a `git merge` of two clones does, hold a
    /// conflict; the next write to that register depends on both sides, and
    /// nothing else is written.
    #[test]
    fn the_next_append_settles_a_union() {
        let here = Store::new(scratch("weave-here"), roster());
        let there = Store::new(scratch("weave-there"), roster());
        let genesis = here.init("union").expect("begins");
        there.adopt(&here, &genesis).expect("adopts the genesis");
        let mine = here
            .append(mk_completed("alpha", at(2), "bassel", "mine").expect("valid"))
            .expect("appends");
        let theirs = there
            .append(mk_completed("alpha", at(2), "bassel", "theirs").expect("valid"))
            .expect("appends");
        for entry in fs::read_dir(there.objects_dir()).expect("lists") {
            let path = entry.expect("an entry").path();
            let file = path.file_name().expect("a file name");
            fs::copy(&path, here.objects_dir().join(file)).expect("copies");
        }
        let settled = here
            .append(mk_reopened("alpha", at(3), "bassel", "both").expect("valid"))
            .expect("appends");
        let mut both = vec![mine, theirs];
        both.sort();
        assert_eq!(change(&here, &settled).deps, both);
        assert_eq!(tips(&here), [settled].into_iter().collect());
        assert_eq!(findings(&here), Vec::<String>::new());
        let _ = fs::remove_dir_all(here.root());
        let _ = fs::remove_dir_all(there.root());
    }

    /// NO STORED BYTE MOVES, AND THE OLD FILES ARE SAID TO GO. A store laid out
    /// the way 0.8 wrote it — HEAD naming the tip, `refs/` naming both heads
    /// while there were two — derives exactly the heads those files named, and
    /// `verify` names each file as a leftover rather than deleting it.
    #[test]
    fn an_old_store_derives_what_its_head_named_and_is_told_to_drop_it() {
        let store = Store::new(scratch("legacy"), roster());
        let first = seal(
            &store,
            mk_created("alpha", at(1), "bassel", "", "").expect("valid"),
        );
        let second = seal(
            &store,
            mk_completed("alpha", at(2), "bassel", "").expect("valid"),
        );
        let aside = put(
            &store,
            mk_sealed(
                Some(first),
                mk_created("beta", at(2), "bassel", "", "").expect("valid"),
            ),
        )
        .expect("writes");
        fs::write(store.root().join("HEAD"), second.as_str()).expect("writes HEAD");
        let refs = store.root().join("refs");
        fs::create_dir_all(&refs).expect("creates refs/");
        for head in [&second, &aside] {
            fs::write(refs.join(head.as_str()), head.as_str()).expect("writes a ref");
        }
        assert_eq!(tips(&store), [second, aside].into_iter().collect());
        assert_eq!(
            findings(&store),
            vec![
                "HEAD is left from before tips were derived (SPEC §3): nothing reads it, delete it",
                "refs/ is left from before tips were derived (SPEC §3): nothing reads it, delete it",
            ]
        );
        fs::remove_file(store.root().join("HEAD")).expect("removes HEAD");
        fs::remove_dir_all(&refs).expect("removes refs/");
        assert_eq!(findings(&store), Vec::<String>::new());
        let _ = fs::remove_dir_all(store.root());
    }

    /// THE VECTOR FOR `adopt_objects`: a replica whose objects arrive as BYTES
    /// lands exactly where the same replica's DIRECTORY would have landed.
    ///
    /// Two stores are built with the same history and diverged the same way;
    /// one takes the other in through [`EventStore::adopt`] and the other
    /// through [`EventStore::adopt_objects`], and the heads, the objects and
    /// `verify` agree. That is the whole claim the new door makes — same
    /// placement, same reverification, a different source of bytes — and the
    /// two refusals below are the other half of it.
    #[test]
    fn adopting_bytes_lands_where_adopting_a_directory_lands() {
        let by_dir = Store::new(scratch("bytes-dir"), roster());
        let by_bytes = Store::new(scratch("bytes-map"), roster());
        let there = Store::new(scratch("bytes-there"), roster());
        let shared = mk_created("alpha", at(1), "bassel", "", "").expect("valid");
        for store in [&by_dir, &by_bytes, &there] {
            seal(store, shared.clone());
        }
        // Each side writes its own second object, so the two histories diverge.
        for store in [&by_dir, &by_bytes] {
            seal(
                store,
                mk_created("beta", at(2), "bassel", "mine", "").expect("valid"),
            );
        }
        let theirs = seal(
            &there,
            mk_created("beta", at(2), "bassel", "theirs", "").expect("valid"),
        );

        by_dir.adopt(&there, &theirs).expect("adopts a directory");
        // The same objects, as the wire carries them: a name and the canonical
        // print that name is the hash of.
        let over_the_wire: BTreeMap<Hash, String> = there
            .dag()
            .expect("reads")
            .objects()
            .iter()
            .map(|(name, object)| (name.clone(), crate::event::canonical_envelope(object)))
            .collect();
        by_bytes
            .adopt_objects(&over_the_wire, &theirs)
            .expect("adopts bytes");

        assert_eq!(tips(&by_bytes), tips(&by_dir));
        assert_eq!(by_bytes.dag().expect("reads"), by_dir.dag().expect("reads"));
        assert_eq!(findings(&by_bytes), Vec::<String>::new());

        // A REPLICA IS NOT TRUSTED FOR BEING A REPLICA. Bytes that do not hash
        // to the name they were filed under are refused, and so is a parent
        // nobody holds.
        let liar = Store::new(scratch("bytes-liar"), roster());
        let mut tampered = over_the_wire.clone();
        if let Some(text) = tampered.get_mut(&theirs) {
            text.push(' ');
        }
        assert!(liar.adopt_objects(&tampered, &theirs).is_err());
        let orphan: BTreeMap<Hash, String> = over_the_wire
            .iter()
            .filter(|(name, _)| **name == theirs)
            .map(|(name, text)| (name.clone(), text.clone()))
            .collect();
        let missing = Store::new(scratch("bytes-orphan"), roster());
        assert!(missing.adopt_objects(&orphan, &theirs).is_err());
        // And a refused adoption WROTE NOTHING: with derived tips an object is
        // in the store the moment its file is, so the tip whose parent never
        // arrived must not have landed on its own.
        assert_eq!(tips(&liar), BTreeSet::new());
        assert_eq!(tips(&missing), BTreeSet::new());

        for store in [&by_dir, &by_bytes, &there, &liar, &missing] {
            let _ = fs::remove_dir_all(store.root());
        }
    }

    #[test]
    fn a_contained_tip_changes_nothing_and_a_containing_one_fast_forwards() {
        let here = Store::new(scratch("ff-here"), roster());
        let there = Store::new(scratch("ff-there"), roster());
        let first = seal(
            &here,
            mk_created("alpha", at(1), "bassel", "", "").expect("valid"),
        );
        there.adopt(&here, &first).expect("adopts");
        let second = seal(
            &there,
            mk_completed("alpha", at(2), "bassel", "").expect("valid"),
        );
        here.adopt(&there, &second).expect("fast-forwards");
        assert_eq!(tips(&here), [second.clone()].into_iter().collect());
        here.adopt(&there, &first).expect("a contained tip");
        assert_eq!(tips(&here), [second].into_iter().collect());
        let _ = fs::remove_dir_all(here.root());
        let _ = fs::remove_dir_all(there.root());
    }

    #[test]
    fn a_tampered_object_is_caught_on_read_and_by_verify() {
        let store = Store::new(scratch("tamper"), roster());
        let digest = seal(
            &store,
            mk_created("alpha", at(1), "bassel", "", "").expect("valid"),
        );
        seal(
            &store,
            mk_completed("alpha", at(2), "bassel", "").expect("valid"),
        );
        let path = store.object_path(&digest);
        let text = fs::read_to_string(&path).expect("reads");
        fs::write(&path, text.replace("alpha", "omega")).expect("writes");
        assert!(store.load(&digest).is_err());
        // The reads ANSWER, and say what they left out: the file, with why,
        // and the object resting on it. A FILE CHANGED UNDER A HELD NAME IS
        // READ AGAIN, so the handle that verified the object before it was
        // tampered with reads as a fresh one does.
        let fresh = Store::new(store.root(), roster());
        assert!(
            fresh.dag().expect("reads").objects().is_empty(),
            "the history is what rests on nothing unread"
        );
        for at in [&fresh, &store] {
            assert!(at.tips().expect("answers").is_empty());
            let excluded = at.dag().expect("reads").excluded();
            assert_eq!(excluded.len(), 1);
            assert_eq!(excluded[0].name, digest);
            assert!(matches!(excluded[0].why, Some(Unread::Tampered(_))));
            assert_eq!(excluded[0].resting.len(), 1);
        }
        // A WRITE REFUSES: the file it cannot read might be a head.
        let refusal = store
            .append(mk_reopened("alpha", at(3), "bassel", "").expect("valid"))
            .expect_err("a write does not guess")
            .to_string();
        assert!(refusal.contains(digest.as_str()), "{refusal}");
        // `verify` REPORTS, twice over, and both are true: the file no longer
        // hashes to its name, and the object resting on it rests on nothing.
        assert_eq!(
            findings(&store),
            vec![
                format!(
                    "object {} does not hash to its filename (tampered/corrupt)",
                    digest.as_str()
                ),
                format!(
                    "chain broke at {digest}: object {digest} hashes to {} — tampered or \
                     corrupt: `prodrome fsck` (`EventStore::fsck`) sets it aside so the store \
                     writes again",
                    Hash::of_bytes(&fs::read(&path).expect("reads")).as_str(),
                    digest = digest.as_str()
                ),
            ]
        );
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn an_unconfirmed_event_dated_behind_its_predecessor_is_a_finding() {
        let store = Store::new(scratch("dating"), roster());
        seal(
            &store,
            mk_created("alpha", at(10), "bassel", "", "").expect("valid"),
        );
        let late = seal(
            &store,
            mk_completed("alpha", at(2), "triage", "").expect("valid"),
        );
        assert_eq!(
            findings(&store),
            vec![format!(
                "untrusted event {} is dated {}, behind its predecessor ({})",
                late.as_str(),
                at(2).isoformat(),
                at(10).isoformat()
            )]
        );
        // The same event from a writer the policy CONFIRMS is a backfill, not a
        // finding.
        let trusted = Store::new(scratch("dating-trusted"), roster());
        seal(
            &trusted,
            mk_created("alpha", at(10), "bassel", "", "").expect("valid"),
        );
        seal(
            &trusted,
            mk_completed("alpha", at(2), "bassel", "").expect("valid"),
        );
        assert_eq!(findings(&trusted), Vec::<String>::new());
        let _ = fs::remove_dir_all(store.root());
        let _ = fs::remove_dir_all(trusted.root());
    }

    /// THE CLOCK RULE IS `confirms`, NOT `binds`. A record from an actor on the
    /// roster BINDS — the folds take it (§5) — and its stamp is still the
    /// host's, so a record dated behind what it rests on is still a finding.
    /// The reference policy is the one that draws that line, by overriding
    /// `Policy::confirms`; `conformance/dag.py` holds these findings for
    /// records too, which is why the distinction is pinned here and not left to
    /// the default.
    #[test]
    fn a_record_that_binds_is_still_held_to_the_dag_s_clock() {
        let store = Store::new(scratch("dating-record"), roster());
        seal(
            &store,
            mk_created("alpha", at(10), "bassel", "", "").expect("valid"),
        );
        let record = seal(
            &store,
            crate::reference::mk_authored(
                "alpha",
                at(2),
                "triage",
                "todo",
                at(2),
                "body",
                None,
                vec![],
                "",
                "",
                "",
                None,
                vec![],
                vec![],
                "",
            )
            .expect("valid"),
        );
        let events = store.events().expect("reads");
        let late = events.last().expect("the record");
        assert!(
            roster()
                .standing(&Hash::new("0".repeat(64)).expect("a name"), late)
                .binds(),
            "a content record binds whoever wrote it"
        );
        assert_eq!(
            findings(&store),
            vec![format!(
                "untrusted event {} is dated {}, behind its predecessor ({})",
                record.as_str(),
                at(2).isoformat(),
                at(10).isoformat()
            )]
        );
        let _ = fs::remove_dir_all(store.root());
    }

    /// THE MEMORY FOLLOWS THE LISTING. What the directory no longer lists
    /// the memory forgets, so a host that deletes an object — the one removal
    /// a host does, a rejected proposal nothing rests on — sees its parent
    /// become a tip again through the same handle.
    #[test]
    fn the_memory_follows_the_listing() {
        let store = Store::new(scratch("index"), roster());
        let first = seal(
            &store,
            mk_created("alpha", at(1), "bassel", "", "").expect("valid"),
        );
        let second = seal(
            &store,
            mk_completed("alpha", at(2), "bassel", "").expect("valid"),
        );
        assert_eq!(tips(&store), [second.clone()].into_iter().collect());
        fs::remove_file(store.object_path(&second)).expect("removes");
        assert_eq!(tips(&store), [first].into_iter().collect());
        let _ = fs::remove_dir_all(store.root());
    }

    /// THERE ARE NO ORPHANS. An object nothing names is not unreachable, it is
    /// a TIP: here a second genesis, what adopting an unrelated replica makes.
    /// It is folded like any other, and the next append joins it.
    #[test]
    fn an_object_nothing_names_is_a_tip() {
        let store = Store::new(scratch("orphan"), roster());
        let first = seal(
            &store,
            mk_created("alpha", at(1), "bassel", "", "").expect("valid"),
        );
        let other = put(
            &store,
            mk_sealed(
                None,
                mk_created("beta", at(1), "bassel", "", "").expect("valid"),
            ),
        )
        .expect("writes");
        assert_eq!(tips(&store), [first, other].into_iter().collect());
        assert_eq!(findings(&store), Vec::<String>::new());
        assert_eq!(store.events().expect("reads").len(), 2);
        let _ = fs::remove_dir_all(store.root());
    }

    proptest! {
        #![proptest_config(crate::cases::cases(32))]

        /// A CRASH BETWEEN WRITE AND RENAME LEAVES NO OBJECT, only a temp that
        /// `verify` reports. A chain of `crash` events is written, and the
        /// next object's placement stops after its temp is written and synced
        /// and before the rename — the crash. The store is then exactly the
        /// chain: the tips derive from it, the object the crash interrupted is
        /// not there under any name, and `verify`'s one finding is the temp,
        /// by name.
        #[test]
        fn a_crash_before_the_rename_leaves_only_a_temp(
            crash in 0usize..5
        ) {
            let store = Store::new(scratch("crash"), roster());
            let mut written: Vec<Hash> = Vec::new();
            for day in 0..crash {
                let day = u32::try_from(day).expect("a few days") + 1;
                written.push(
                    seal(&store, mk_reopened("alpha", at(day), "bassel", "").expect("valid")),
                );
            }
            let day = u32::try_from(crash).expect("a few days") + 1;
            let next: Envelope<TodoEvent<Todo>> = mk_sealed(
                written.last().cloned(),
                mk_reopened("alpha", at(day), "bassel", "").expect("valid"),
            );
            fs::create_dir_all(store.objects_dir()).expect("creates objects/");
            let before = Store::new(store.root(), roster()).dag().map(|dag| (*dag).clone());
            let temp = stage(
                &store.objects_dir(),
                crate::event::canonical_envelope(&next).as_bytes(),
            )
            .expect("stages");
            let file = temp.file_name().expect("a name").to_string_lossy().into_owned();

            let fresh = Store::new(store.root(), roster());
            let tips = fresh.tips();
            let after = fresh.dag().map(|dag| (*dag).clone());
            let landed = store.object_path(&seal_hash(&next)).exists();
            let found = findings(&store);
            let _ = fs::remove_dir_all(store.root());
            prop_assert_eq!(
                after.expect("the store reads"),
                before.expect("the store read"),
                "the store reads as it did before the interrupted write"
            );
            prop_assert_eq!(
                tips.expect("the tips derive"),
                written.last().cloned().into_iter().collect::<BTreeSet<_>>()
            );
            prop_assert!(!landed, "the interrupted object is not in the store");
            prop_assert_eq!(
                found,
                vec![format!(
                    "objects/{file} is not an object: a temp an interrupted write left, or a \
                     stray (SPEC §3); delete it"
                )]
            );
        }
    }

    /// THE STORE'S OWN `.gitattributes` IS WRITTEN AS AN OBJECT IS. A crash
    /// between its temp and its rename leaves the root's temp, which
    /// `verify` names, and nothing else changed: the store reads as before,
    /// and the next write puts the file in place. One a host already wrote
    /// is the host's, and no write rewrites it.
    #[test]
    fn the_gitattributes_is_written_durably_and_never_rewritten() {
        let store = Store::new(scratch("gitattributes"), roster());
        store.init("attributes").expect("begins");
        let path = store.root().join(".gitattributes");
        fs::remove_file(&path).expect("removes it, to write it again");
        let before = store.dag().expect("reads");
        let temp = stage(store.root(), GITATTRIBUTES.as_bytes()).expect("stages");
        let file = temp
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        assert!(is_temp(&file));
        assert_eq!(
            Store::new(store.root(), roster()).dag().expect("reads"),
            before
        );
        assert_eq!(
            findings(&store),
            vec![format!(
                "{file} is not an object: a temp an interrupted write left, or a stray (SPEC §3); \
                 delete it"
            )]
        );
        fs::remove_file(&temp).expect("deletes the temp");
        store
            .append(mk_completed("alpha", at(2), "writer", "").expect("valid"))
            .expect("appends");
        assert_eq!(fs::read_to_string(&path).expect("written"), GITATTRIBUTES);
        fs::write(&path, "* -text\n").expect("the host's own");
        store
            .append(mk_reopened("alpha", at(3), "writer", "").expect("valid"))
            .expect("appends");
        assert_eq!(fs::read_to_string(&path).expect("kept"), "* -text\n");
        assert_eq!(findings(&store), Vec::<String>::new());
        let _ = fs::remove_dir_all(store.root());
    }

    /// A DIRECTORY THE STORE MAKES IS MADE DURABLY, every missing ancestor
    /// synced into its parent, and one that exists is left as it is.
    #[test]
    fn a_directory_is_made_with_its_ancestors() {
        let root = scratch("dirs");
        let deep = root.join("a").join("b").join("objects");
        make_dir(&deep).expect("makes it");
        assert!(deep.is_dir());
        make_dir(&deep).expect("a second make is nothing");
        put_durably(&root.join("a").join("file"), b"bytes").expect("writes");
        assert_eq!(
            fs::read(root.join("a").join("file")).expect("reads"),
            b"bytes"
        );
        assert_eq!(
            fs::read_dir(root.join("a"))
                .expect("lists")
                .filter_map(Result::ok)
                .filter(|entry| is_temp(&entry.file_name().to_string_lossy()))
                .count(),
            0,
            "no temp is left behind a finished write"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// An entry of `objects/` that could be taken for an object and is not
    /// one: a `.tmp`, a name one character short or in capitals, a `.py` of
    /// anything, a file with no suffix. Drawn and then filtered, so a draw
    /// that happens to be a well-formed object name is not a stray.
    fn a_stray() -> impl Strategy<Value = String> {
        prop_oneof![
            "[0-9a-f]{64}\\.tmp",
            "[0-9a-f]{63}\\.py",
            "[0-9A-F]{64}\\.py",
            "[a-z0-9_.-]{1,12}\\.py",
            "[a-zA-Z0-9_-]{1,70}",
        ]
        .prop_filter("an object name is not a stray", |file| {
            file.strip_suffix(".py")
                .map(Hash::new)
                .is_none_or(|name| name.is_err())
        })
    }

    proptest! {
        #![proptest_config(crate::cases::cases(64))]

        /// VERIFY NAMES EVERY STRAY, and the reads pass over them. Whatever
        /// lands in `objects/` beside a healthy store's objects, `verify`
        /// reports each such entry once, by name, in name order, and nothing
        /// else; the tips and the objects read exactly as they did.
        #[test]
        fn verify_names_every_stray(strays in prop::collection::btree_set(a_stray(), 1..6)) {
            let store = Store::new(scratch("strays"), roster());
            seal(&store, mk_created("alpha", at(1), "bassel", "", "").expect("valid"));
            let tip = seal(&store, mk_completed("alpha", at(2), "bassel", "").expect("valid"));
            for file in &strays {
                fs::write(store.objects_dir().join(file), "stray").expect("writes a stray");
            }
            let found = findings(&store);
            let expected: Vec<String> = strays
                .iter()
                .map(|file| {
                    format!(
                        "objects/{file} is not an object: a temp an interrupted write left, or a \
                         stray (SPEC §3); delete it"
                    )
                })
                .collect();
            let fresh = Store::new(store.root(), roster());
            let tips = fresh.tips();
            let read = fresh.dag().map(|dag| dag.objects().len());
            let _ = fs::remove_dir_all(store.root());
            prop_assert_eq!(found, expected);
            prop_assert_eq!(tips.expect("the tips derive"), [tip].into_iter().collect::<BTreeSet<_>>());
            prop_assert_eq!(read.expect("the store reads"), 2);
        }
    }

    /// ONE BAD OBJECT DOES NOT STOP EVERYTHING. A chain of three whose middle
    /// file is damaged: a fresh handle's `tips` answers the first object,
    /// what verifies and rests on nothing unread, and every append refuses,
    /// naming the object and the command that sets it aside. Once it is
    /// quarantined the store reads the largest down-set it holds: the chain
    /// without the damaged object and without the object resting on it, so
    /// the first object is the one tip and the only head an append sees.
    /// `verify` is clean but for the receipt, which stands in for the "chain
    /// broke" the waiting object would be. Restored, the object brings the
    /// one resting on it back, a write concurrent with the append, and
    /// `verify` is clean.
    /// A healthy object is not quarantined: that would be hiding it.
    #[test]
    fn after_quarantine_the_store_answers_and_verify_holds_the_receipt() {
        let store = Store::new(scratch("quarantine"), roster());
        let first = seal(
            &store,
            mk_created("alpha", at(1), "bassel", "", "").expect("valid"),
        );
        let middle = seal(
            &store,
            mk_completed("alpha", at(2), "bassel", "").expect("valid"),
        );
        let last = seal(
            &store,
            mk_reopened("alpha", at(3), "bassel", "").expect("valid"),
        );
        let path = store.object_path(&middle);
        let text = fs::read_to_string(&path).expect("reads");
        fs::write(&path, text.replace("alpha", "omega")).expect("damages");

        let fresh = Store::new(store.root(), roster());
        assert_eq!(tips(&fresh), [first.clone()].into_iter().collect());
        let refusal = fresh
            .append(mk_completed("alpha", at(4), "bassel", "").expect("valid"))
            .expect_err("a write does not guess a head")
            .to_string();
        assert!(refusal.contains(middle.as_str()), "{refusal}");
        assert!(refusal.contains("`prodrome fsck`"), "{refusal}");

        let receipt = format!(
            "quarantine/{}.py failed its hash and was set aside (SPEC §3): restore the object \
             from a replica, then delete this file",
            middle.as_str()
        );
        let set_aside = vec![Finding::Quarantined(format!("{}.py", middle.as_str()))];
        assert_eq!(fresh.fsck(), set_aside, "named in the report");
        let aside = store
            .root()
            .join("quarantine")
            .join(format!("{}.py", middle.as_str()));
        assert_eq!(
            fs::read_to_string(&aside).expect("set aside, never deleted"),
            text.replace("alpha", "omega")
        );
        assert!(!path.exists());
        assert!(store.object_path(&first).exists(), "a healthy object stays");
        // `last` rests on what the store lacks, so it is out of the history
        // until `middle` is back: the history is `first` alone.
        assert_eq!(tips(&fresh), [first.clone()].into_iter().collect());
        assert_eq!(
            fresh
                .dag()
                .expect("reads")
                .objects()
                .keys()
                .collect::<Vec<_>>(),
            vec![&first]
        );
        assert_eq!(findings(&fresh), vec![receipt.clone()]);
        assert_eq!(fresh.fsck(), set_aside, "a second fsck moves nothing");
        let next = fresh
            .append(mk_completed("alpha", at(4), "bassel", "").expect("valid"))
            .expect("the store writes again");
        assert_eq!(change(&fresh, &next).genesis, first);
        // `first` is the head of `alpha` in the history the store holds.
        assert_eq!(change(&fresh, &next).deps, vec![first.clone()]);
        assert_eq!(tips(&fresh), [next.clone()].into_iter().collect());
        assert_eq!(findings(&fresh), vec![receipt]);

        // The repair: the object back from a replica, the receipt deleted.
        // `last` rejoins with it, concurrent with `next`, which was written
        // over the history without them: both are tips, and nothing is
        // reported.
        fs::write(&path, text).expect("restores");
        fs::remove_dir_all(store.root().join("quarantine")).expect("deletes the receipt");
        assert_eq!(
            tips(&fresh),
            [last.clone(), next.clone()].into_iter().collect()
        );
        assert!(fresh.concurrent(&last, &next).expect("both held"));
        assert_eq!(findings(&fresh), Vec::<String>::new());
        let _ = fs::remove_dir_all(store.root());
    }

    /// THE LOCK IS THE PYTHON WRITER'S LOCK, and this is the checkable half of
    /// that claim: it is `root/.lock`, and an append takes it exclusively.
    ///
    /// What is checked is that a SECOND holder of an exclusive `flock` on that
    /// exact path cannot get it while an append would hold one, and that the
    /// append is possible once that holder lets go — which is what serialises
    /// this writer against the reference's `store.py::_flock` across
    /// processes. The file NAME is the interoperable part: two writers locking
    /// two different files are two writers that do not lock.
    #[cfg(unix)]
    #[test]
    fn an_append_takes_the_same_lock_file_the_python_writer_takes() {
        use rustix::fs::{flock, FlockOperation};

        let store = Store::new(scratch("locked"), roster());
        store.init("locked").expect("begins");
        assert_eq!(store.lock_file(), store.root().join(".lock"));
        assert!(store.lock_file().exists(), "the write created the lock");

        // A second exclusive holder, the way another process would be.
        let held = File::create(store.lock_file()).expect("the lock file opens");
        flock(&held, FlockOperation::LockExclusive).expect("locks");
        assert!(
            flock(
                File::create(store.lock_file()).expect("opens"),
                FlockOperation::NonBlockingLockExclusive
            )
            .is_err(),
            "a held exclusive lock is not available to a second holder"
        );
        drop(held);
        store
            .append(mk_completed("alpha", at(2), "bassel", "").expect("valid"))
            .expect("appends once the other writer let go");
        assert_eq!(findings(&store), Vec::<String>::new());
        let _ = fs::remove_dir_all(store.root());
    }
}
