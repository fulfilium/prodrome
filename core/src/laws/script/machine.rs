//! The machine that runs a script: three replicas, what each answered it
//! took, the readings recorded at down-sets, a cache of readings, and the
//! script's account of keys and faults. [`Machine::step`] takes one step;
//! [`Machine::check`] checks every invariant.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::kept::{Kept, Scratch};
use super::keys::{root, roots, secret, Keys};
use super::reading::{instant, Reading};
use super::step::{Refusal, Root, Step, Through, Who, Writing};
use crate::change::mk_change;
use crate::dag::{Dag, Finding};
use crate::event::{canonical_envelope, Envelope, Hash};
use crate::fold::{read, Product};
use crate::fpl::Instant;
use crate::genesis::mk_genesis;
use crate::laws::reading::Registers;
use crate::laws::Broken;
use crate::literal::ProdromeError;
use crate::memo::{self, Cache, Key, Lookup, Lru};
use crate::policy::{Everything, Policy};
use crate::registers::{deps_for, extend, fold, heads, Folded};
use crate::schema::Schema;
use crate::sign::{mk_key_revoked, Checks, KeyAdded, Proof, Registrar, Signed};
use crate::snapshot::{attest, mk_snapshot, Attestation};
use crate::store::{accept, Held, Replica};

/// How many readings the cache keeps: few, so that it evicts.
const KEPT: usize = 6;

/// What a replica answered it took, applied to a set of names and a fold of
/// their own: the state the answers say the replica holds.
struct Shadow<E: Schema> {
    names: BTreeSet<Hash>,
    folded: Folded<E>,
}

impl<E: Schema> Shadow<E> {
    fn empty() -> Self {
        Shadow {
            names: BTreeSet::new(),
            folded: Folded::empty(),
        }
    }
}

/// A reading recorded at a down-set: the step that recorded it, the tips,
/// the instant and what it read.
struct Record<E: Schema> {
    step: usize,
    tips: BTreeSet<Hash>,
    at: Option<Instant>,
    reading: Reading<E>,
}

/// One replica as a check sees it: its objects, its tips, its history and
/// what it reads ever.
struct Seen<E: Schema> {
    label: String,
    tips: BTreeSet<Hash>,
    dag: Arc<Dag<E>>,
    folded: Arc<Folded<E>>,
    reading: Reading<E>,
}

pub(super) struct Machine<E: Schema, P> {
    genesis: Hash,
    /// Every print that the script made or a replica wrote.
    wire: BTreeMap<Hash, Vec<u8>>,
    replicas: [Kept<E>; 3],
    /// What each replica answered it took.
    shadows: [Shadow<E>; 3],
    /// What each overlay's base answered it took.
    bases: [Shadow<E>; 3],
    records: Vec<Record<E>>,
    memo: Cache<Reading<E>, Lru>,
    /// The instant each function name of the cache reads at.
    instants: BTreeMap<Hash, Option<Instant>>,
    checks: Checks,
    keys: Keys,
    /// Each fault the script wrote: the object, and what `verify` finds.
    faults: BTreeMap<Hash, Finding>,
    conduct: Conduct,
    /// The host's policy, under which the claimed reading is read.
    policy: P,
    /// What each replica, by its label, held at the last check: its
    /// readings must not fall since (law 32).
    climbed: BTreeMap<String, Arc<Folded<E>>>,
    /// Removed when the machine is dropped, after the replicas.
    _scratch: Scratch,
}

/// A refusal where none is lawful, as a law of the script.
fn failed(law: u32, what: &str) -> impl Fn(ProdromeError) -> Broken + '_ {
    move |refused| Broken::of(law, format!("{what}: {refused}"))
}

/// The objects `replica` holds.
fn names<E: Schema>(replica: &impl Replica<E>) -> Result<BTreeSet<Hash>, Broken> {
    Ok(replica
        .held()
        .map_err(failed(68, "a replica does not read"))?
        .dag
        .objects()
        .keys()
        .cloned()
        .collect())
}

/// An object, its name and its print.
fn sealed<E: Schema>(object: &Envelope<E>) -> (Hash, Vec<u8>) {
    let print = canonical_envelope(object).into_bytes();
    (Hash::of_bytes(&print), print)
}

/// The name of the function "the reading at `at`", for the cache.
fn function(at: Option<Instant>) -> Hash {
    memo::name(format!("laws::script reading at {at:?}").as_bytes(), [])
}

/// The name of the down-set of `tips`, for the cache.
fn frontier(tips: &BTreeSet<Hash>) -> Hash {
    memo::name(b"laws::script frontier", tips)
}

impl<E: Schema, P: Policy<E> + Clone> Machine<E, P> {
    /// Replicas of `kinds` at the schema `vocabulary`, each holding one
    /// genesis, for a script of `conduct`, read under `policy`.
    pub(super) fn new(
        vocabulary: &E::Vocabulary,
        kinds: [super::Kind; 3],
        conduct: Conduct,
        policy: P,
    ) -> Result<Self, Broken> {
        let scratch = Scratch::new();
        let replicas = [0, 1, 2].map(|n| Kept::new(kinds[n], vocabulary, &scratch, n));
        let genesis: Envelope<E> = Envelope::Genesis(
            mk_genesis("script", &"ab".repeat(16)).map_err(failed(68, "a genesis"))?,
        );
        let (name, print) = sealed(&genesis);
        let mut machine = Machine {
            genesis: name.clone(),
            wire: [(name.clone(), print)].into(),
            replicas,
            shadows: [Shadow::empty(), Shadow::empty(), Shadow::empty()],
            bases: [Shadow::empty(), Shadow::empty(), Shadow::empty()],
            records: Vec::new(),
            memo: Cache::new(Lru::new(KEPT)),
            instants: BTreeMap::new(),
            checks: Checks::new(1 << 12),
            keys: Keys::default(),
            faults: BTreeMap::new(),
            conduct,
            policy,
            climbed: BTreeMap::new(),
            _scratch: scratch,
        };
        for who in Who::ALL {
            machine.receive(who, [name.clone()].into())?;
        }
        Ok(machine)
    }

    /// The objects the replicas hold, together.
    pub(super) fn objects(&self) -> Result<usize, Broken> {
        let mut all = BTreeSet::new();
        for replica in &self.replicas {
            all.extend(names(replica)?);
        }
        Ok(all.len())
    }

    /// Take `step`, the `n`-th.
    pub(super) fn step(&mut self, n: usize, step: &Step<E>) -> Result<(), Broken> {
        match step {
            Step::Append {
                by,
                of,
                event,
                writing,
            } => self.append(*by, *of, event, *writing),
            Step::Pick { from, to, chosen } => {
                let theirs: Vec<Hash> = names(&self.replicas[from.index()])?.into_iter().collect();
                for at in chosen {
                    if let Some(name) = theirs.get(at % theirs.len().max(1)) {
                        self.accept(*from, *to, [name.clone()].into())?;
                    }
                }
                Ok(())
            }
            Step::Sync { from, to } => {
                let tips = self.replicas[from.index()]
                    .tips()
                    .map_err(failed(35, "a replica has no tips"))?;
                self.accept(*from, *to, tips)
            }
            Step::SyncAll => self.sync_all(),
            Step::Refuse { to, refusal, with } => self.refuse(*to, *refusal, *with),
            Step::Rotate { by } => self.rotate(*by),
            Step::Revoke { by, of, key, root } => self.revoke(*by, *of, *key, *root),
            Step::Read { by, at, through } => self.read(*by, *at, *through),
            Step::Record { by, at } => {
                let held = self.held(*by)?;
                let at = instant(&held.dag, *at);
                let reading = Reading::at_down_set(&held.dag, &held.tips, at, &self.policy)
                    .map_err(|what| Broken::of(43, what))?;
                self.records.push(Record {
                    step: n,
                    tips: held.tips,
                    at,
                    reading,
                });
                Ok(())
            }
            Step::Snapshot { by } => self.snapshot(n, *by),
            Step::Forget { key } => {
                let keys: Vec<Key> = self.memo.keys().collect();
                if let Some(doomed) = keys.get(key % keys.len().max(1)) {
                    self.memo
                        .forget(&[doomed.clone()].into())
                        .map_err(failed(55, "a forget"))?;
                    if !matches!(self.memo.get(doomed), Lookup::Miss) {
                        return Err(Broken::of(55, "a forgotten key still reads"));
                    }
                }
                Ok(())
            }
            Step::Flush { by } => {
                let kept = &self.replicas[by.index()];
                let Some(flushed) = kept.flush() else {
                    return Ok(());
                };
                let taken = flushed.map_err(failed(35, "a flush"))?;
                let again = match kept.flush() {
                    Some(flushed) => flushed.map_err(failed(35, "a second flush"))?,
                    None => Vec::new(),
                };
                self.emit(*by, Layer::Base, &taken, &again)
            }
        }
    }

    fn held(&self, who: Who) -> Result<Held<E>, Broken> {
        self.replicas[who.index()]
            .held()
            .map_err(failed(68, "a replica does not read"))
    }

    /// `to` receives `seeds` from the wire, twice.
    fn receive(&mut self, to: Who, seeds: BTreeSet<Hash>) -> Result<(), Broken> {
        let replica = &self.replicas[to.index()];
        let wire = &self.wire;
        let taken = replica
            .receive(seeds.clone(), &|name| wire.get(name).cloned())
            .map_err(failed(35, "a receipt of the script's own objects"))?;
        let again = replica
            .receive(seeds, &|name| wire.get(name).cloned())
            .map_err(failed(35, "a second receipt"))?;
        self.emit(to, Layer::Replica, &taken, &again)
    }

    /// `to` receives `chosen` from `from`, twice.
    fn accept(&mut self, from: Who, to: Who, chosen: BTreeSet<Hash>) -> Result<(), Broken> {
        let (source, target) = (&self.replicas[from.index()], &self.replicas[to.index()]);
        let taken = accept(source, target, chosen.clone()).map_err(failed(35, "a sync"))?;
        let again = accept(source, target, chosen).map_err(failed(35, "a second sync"))?;
        self.emit(to, Layer::Replica, &taken, &again)
    }

    /// Apply what a replica answered it took to its shadow. A second
    /// receipt of the same objects must take nothing, and no answer may
    /// name an object the replica held before.
    fn emit(
        &mut self,
        who: Who,
        layer: Layer,
        taken: &[Hash],
        again: &[Hash],
    ) -> Result<(), Broken> {
        if !again.is_empty() {
            return Err(Broken::of(35, format!("a second receipt took {again:?}")));
        }
        let dag = match (layer, self.replicas[who.index()].base()) {
            (Layer::Base, Some(base)) => base.held(),
            (Layer::Replica | Layer::Base, _) => self.replicas[who.index()].held(),
        }
        .map_err(failed(68, "a replica does not read"))?
        .dag;
        let shadow = match layer {
            Layer::Replica => &mut self.shadows[who.index()],
            Layer::Base => &mut self.bases[who.index()],
        };
        if let Some(held) = taken.iter().find(|name| shadow.names.contains(*name)) {
            return Err(Broken::of(
                68,
                format!("a receipt answered {held}, which the replica held"),
            ));
        }
        let nodes: Vec<_> = taken.iter().filter_map(|name| dag.node(name)).collect();
        if nodes.len() != taken.len() {
            return Err(Broken::of(
                68,
                format!("a receipt answered {taken:?}, which the replica does not all hold"),
            ));
        }
        shadow.names.extend(taken.iter().cloned());
        shadow.folded = extend(&shadow.folded, &nodes);
        Ok(())
    }

    fn append(&mut self, by: Who, of: Who, event: &E, writing: Writing) -> Result<(), Broken> {
        let name = match writing {
            Writing::Foreign { dep } => match self.foreign(by, event, dep)? {
                Some(name) => Some(name),
                None => self.appended(by, event)?,
            },
            Writing::Below => match self.below(by, event)? {
                Some(name) => Some(name),
                None => self.appended(by, event)?,
            },
            Writing::Signed | Writing::Unsigned | Writing::Forged => self.appended(by, event)?,
        };
        let Some(name) = name else {
            return Ok(());
        };
        match writing {
            Writing::Signed | Writing::Foreign { .. } | Writing::Below => {
                self.sign(by, of, &name, true)
            }
            Writing::Forged => self.sign(by, of, &name, false),
            Writing::Unsigned => Ok(()),
        }
    }

    /// `by` appends `event`, twice: the second append answers the first's
    /// name and writes nothing (law 20). `None` where the append refuses,
    /// as §3 lets an append refuse, and then the replica holds what it
    /// held.
    fn appended(&mut self, by: Who, event: &E) -> Result<Option<Hash>, Broken> {
        let replica = &self.replicas[by.index()];
        let before = names(replica)?;
        let Ok(name) = replica.append(event.clone()) else {
            return if names(replica)? == before {
                Ok(None)
            } else {
                Err(Broken::of(20, "a refused append wrote an object"))
            };
        };
        let after = names(replica)?;
        let again = replica
            .append(event.clone())
            .map_err(failed(20, "an append again"))?;
        if again != name || names(replica)? != after {
            return Err(Broken::of(
                20,
                format!("an append again answered {again}, not {name}, or wrote"),
            ));
        }
        let fresh: Vec<Hash> = after.difference(&before).cloned().collect();
        let lawful = if fresh.is_empty() {
            before.contains(&name)
        } else {
            fresh == [name.clone()]
        };
        if !lawful {
            return Err(Broken::of(
                20,
                format!("an append answered {name} and wrote {fresh:?}"),
            ));
        }
        if let Some(print) = replica.print(&name) {
            self.wire.insert(name.clone(), print);
        }
        self.emit(by, Layer::Replica, &fresh, &[])?;
        Ok(Some(name))
    }

    /// `by` writes `event` as a change whose deps are its entity's heads
    /// and the `dep`-th tip that is a write to another entity none of whose
    /// past is a write to `event`'s entity. `None` where no tip is one.
    fn foreign(&mut self, by: Who, event: &E, dep: usize) -> Result<Option<Hash>, Broken> {
        let held = self.held(by)?;
        let prodrome = held.dag.key(&self.genesis);
        let Ok(mut deps) = deps_for(&held.folded, &prodrome, event) else {
            return Ok(None);
        };
        let ours = |name: &Hash| {
            held.dag
                .get(name)
                .and_then(Envelope::event)
                .is_some_and(|written| written.key() == event.key())
        };
        let others: Vec<&Hash> = held
            .tips
            .iter()
            .filter(|tip| matches!(held.dag.get(tip), Some(Envelope::Change(_))))
            .filter(|tip| !held.dag.closure([(*tip).clone()]).iter().any(ours))
            .collect();
        let Some(other) = others.get(dep % others.len().max(1)) else {
            return Ok(None);
        };
        deps.push((*other).clone());
        let change = Envelope::Change(
            mk_change(self.genesis.clone(), deps, event.clone())
                .map_err(failed(68, "a change with a foreign dep"))?,
        );
        let (name, print) = sealed(&change);
        self.wire.insert(name.clone(), print);
        self.faults.insert(
            name.clone(),
            Finding::Foreign {
                change: name.clone(),
                dep: (*other).clone(),
            },
        );
        self.receive(by, [name.clone()].into())?;
        Ok(Some(name))
    }

    /// `by` writes `event` as a change whose deps are its entity's heads,
    /// where the append refuses `event` because it does not stand on the
    /// reading of an inflationary register, the empty reading of an entity
    /// with no write included. The replica receives the change, as it
    /// receives any valid object. `None` where the append takes `event`.
    fn below(&mut self, by: Who, event: &E) -> Result<Option<Hash>, Broken> {
        let held = self.held(by)?;
        let prodrome = held.dag.key(&self.genesis);
        let Err(why) = deps_for(&held.folded, &prodrome, event) else {
            return Ok(None);
        };
        let deps = heads(held.folded.stream(&prodrome, event.key()));
        let change = Envelope::Change(
            mk_change(self.genesis.clone(), deps, event.clone())
                .map_err(failed(68, "a change below its reading"))?,
        );
        let (name, print) = sealed(&change);
        self.wire.insert(name.clone(), print);
        self.faults.insert(
            name.clone(),
            Finding::Below {
                write: name.clone(),
                why,
            },
        );
        self.receive(by, [name.clone()].into())?;
        Ok(Some(name))
    }

    /// `by` signs `object` with `of`'s current key, or, where `verifies` is
    /// false, gives it a signature by that key (or by `of`'s first key,
    /// where it has none) that does not verify.
    fn sign(&mut self, by: Who, of: Who, object: &Hash, verifies: bool) -> Result<(), Broken> {
        let (generation, key) = match (self.keys.current(of), verifies) {
            (Some(current), _) => current,
            (None, true) => return Ok(()),
            (None, false) => (0, secret(of, 0)),
        };
        let signed = if verifies {
            key.sign(object)
        } else {
            Signed {
                object: object.clone(),
                ..key.sign(&Hash::of_bytes(b"another object"))
            }
        };
        let (name, print) = sealed::<E>(&Envelope::Signed(signed));
        self.wire.insert(name.clone(), print);
        self.keys
            .signed(name.clone(), object.clone(), of, generation, verifies);
        if !verifies {
            self.faults.insert(
                name.clone(),
                Finding::Forged {
                    signed: name.clone(),
                },
            );
        }
        self.receive(by, [name].into())
    }

    fn sync_all(&mut self) -> Result<(), Broken> {
        for (from, to) in [
            (Who::Ben, Who::Ada),
            (Who::Cai, Who::Ada),
            (Who::Ada, Who::Ben),
            (Who::Ada, Who::Cai),
        ] {
            let tips = self.replicas[from.index()]
                .tips()
                .map_err(failed(35, "a replica has no tips"))?;
            self.accept(from, to, tips)?;
        }
        let ada = names(&self.replicas[0])?;
        for who in [Who::Ben, Who::Cai] {
            let theirs = names(&self.replicas[who.index()])?;
            if theirs != ada {
                return Err(Broken::of(
                    35,
                    format!(
                        "after a sync of all, {} {}",
                        who.name(),
                        crate::laws::reading::sets("holds", &theirs, &ada)
                    ),
                ));
            }
        }
        Ok(())
    }

    /// `to` is offered a print it must refuse, beside the `with`-th object
    /// of the wire it lacks: the receipt refuses and takes neither.
    fn refuse(&mut self, to: Who, refusal: Refusal, with: usize) -> Result<(), Broken> {
        let (bad, print) = match refusal {
            Refusal::Tampered => {
                let prints: Vec<&Vec<u8>> = self.wire.values().collect();
                let print = prints[with % prints.len()].clone();
                (Hash::of_bytes(format!("tampered {with}").as_bytes()), print)
            }
            Refusal::Unparsed => {
                let print = format!("Unparsed{with}()").into_bytes();
                (Hash::of_bytes(&print), print)
            }
            Refusal::Orphan => sealed::<E>(&Envelope::Snapshot(
                mk_snapshot(
                    self.genesis.clone(),
                    vec![Hash::of_bytes(format!("absent {with}").as_bytes())],
                    None,
                )
                .map_err(failed(68, "a snapshot"))?,
            )),
        };
        let replica = &self.replicas[to.index()];
        let before = names(replica)?;
        let lacked: Vec<&Hash> = self
            .wire
            .keys()
            .filter(|name| !before.contains(*name))
            .collect();
        let mut seeds: BTreeSet<Hash> = [bad.clone()].into();
        seeds.extend(
            lacked
                .get(with % lacked.len().max(1))
                .map(|name| (*name).clone()),
        );
        let wire = &self.wire;
        let offered = replica.receive(seeds, &|name| {
            if *name == bad {
                Some(print.clone())
            } else {
                wire.get(name).cloned()
            }
        });
        match offered {
            Ok(taken) => Err(Broken::of(
                35,
                format!("a receipt took {taken:?} beside a print it must refuse"),
            )),
            Err(_) if names(replica)? == before => Ok(()),
            Err(_) => Err(Broken::of(35, "a refused receipt took objects")),
        }
    }

    /// `by` revokes its current key, standing behind what it holds, and
    /// vouches again: a new key, registered by the root, signs every object
    /// of `by`'s that `by` holds.
    fn rotate(&mut self, by: Who) -> Result<(), Broken> {
        let held = self.held(by)?;
        let mut seeds = BTreeSet::new();
        if let Some((generation, key)) = self.keys.current(by) {
            let revocation = mk_key_revoked(
                self.genesis.clone(),
                held.tips.iter().cloned().collect(),
                by.actor(),
                key.public(),
            )
            .map_err(failed(68, "a revocation"))?;
            let name = self.rooted(Root::First, &Envelope::KeyRevoked(revocation), &mut seeds);
            let kept = held.dag.objects().keys().cloned().collect();
            self.keys.revoked(name, by, generation, kept);
        }
        let generation = self.keys.next(by);
        let key = secret(by, generation);
        let name = self.rooted(
            Root::First,
            &Envelope::KeyAdded(KeyAdded {
                genesis: self.genesis.clone(),
                actor: by.actor(),
                key: key.public(),
            }),
            &mut seeds,
        );
        self.keys.added(name, by, generation);
        self.receive(by, seeds)?;
        let mine: Vec<Hash> = self
            .held(by)?
            .dag
            .objects()
            .iter()
            .filter(|(_, object)| {
                object
                    .event()
                    .is_some_and(|event| *event.actor() == by.actor())
            })
            .map(|(name, _)| name.clone())
            .collect();
        let mut signatures = BTreeSet::new();
        for object in mine {
            let (name, print) = sealed::<E>(&Envelope::Signed(key.sign(&object)));
            self.wire.insert(name.clone(), print);
            self.keys.signed(name.clone(), object, by, generation, true);
            signatures.insert(name);
        }
        if signatures.is_empty() {
            return Ok(());
        }
        self.receive(by, signatures)
    }

    /// `object`, and `root`'s signature of it, put on the wire and among
    /// `seeds`. Answers the object's name.
    fn rooted(&mut self, root: Root, object: &Envelope<E>, seeds: &mut BTreeSet<Hash>) -> Hash {
        let (name, print) = sealed(object);
        let (signature, signed) = sealed::<E>(&Envelope::Signed(self::root(root).sign(&name)));
        self.wire.insert(name.clone(), print);
        self.wire.insert(signature.clone(), signed);
        seeds.insert(signature);
        seeds.insert(name.clone());
        name
    }

    /// `by` revokes the `key`-th key of `of`'s that it holds as registered,
    /// standing behind what it holds, with `root`'s signature.
    fn revoke(&mut self, by: Who, of: Who, key: usize, root: Root) -> Result<(), Broken> {
        let held = self.held(by)?;
        let generations = self.keys.registered(of, held.dag.objects());
        let Some(&generation) = generations.get(key % generations.len().max(1)) else {
            return Ok(());
        };
        let revocation = mk_key_revoked(
            self.genesis.clone(),
            held.tips.iter().cloned().collect(),
            of.actor(),
            secret(of, generation).public(),
        )
        .map_err(failed(68, "a revocation"))?;
        let mut seeds = BTreeSet::new();
        let name = self.rooted(root, &Envelope::KeyRevoked(revocation), &mut seeds);
        let kept = held.dag.objects().keys().cloned().collect();
        self.keys.revoked(name, of, generation, kept);
        self.receive(by, seeds)
    }

    /// `by` reads at the `at`-th instant, through the cache or afresh. A
    /// reading through the cache is the fresh one (law 51), and a proof
    /// read through the table of checks is the plain proof (law 49).
    fn read(&mut self, by: Who, at: usize, through: Through) -> Result<(), Broken> {
        let held = self.held(by)?;
        let at = instant(&held.dag, at);
        let nodes = held.dag.nodes().map_err(failed(43, "a history"))?;
        let fresh = Reading::of(&held.dag, &fold(&nodes), at, &self.policy);
        let roots = Registrar::Roots(roots().into());
        if Proof::through(&held.dag, &roots, &self.checks) != Proof::of(&held.dag, &roots) {
            return Err(Broken::of(49, "a proof through the table is not the proof"));
        }
        match through {
            Through::Fresh => Ok(()),
            Through::Memo => {
                let key = Key {
                    function: function(at),
                    argument: frontier(&held.tips),
                };
                self.instants.insert(key.function.clone(), at);
                match self.memo.get(&key) {
                    Lookup::Hit(kept) => match kept.differs(&fresh) {
                        Some(difference) => Err(Broken::of(
                            51,
                            format!("a reading through the cache: {difference}"),
                        )),
                        None => Ok(()),
                    },
                    Lookup::Miss => self
                        .memo
                        .insert(key, fresh)
                        .map_err(|finding| Broken::of(57, format!("{finding:?}"))),
                    Lookup::Finding(values) => Err(Broken::of(
                        57,
                        format!("a key holds {} readings", values.len()),
                    )),
                }
            }
        }
    }

    /// `by` writes a snapshot of its tips, and records the reading at it.
    fn snapshot(&mut self, n: usize, by: Who) -> Result<(), Broken> {
        let held = self.held(by)?;
        let snapshot =
            match attest(&held.dag, self.genesis.clone()).map_err(failed(28, "a snapshot"))? {
                Attestation::Standing(_) => return Ok(()),
                Attestation::Fresh(snapshot) => snapshot,
            };
        let (name, print) = sealed::<E>(&Envelope::Snapshot(snapshot));
        self.wire.insert(name.clone(), print);
        self.receive(by, [name.clone()].into())?;
        let held = self.held(by)?;
        let tips: BTreeSet<Hash> = [name].into();
        let reading = Reading::at_down_set(&held.dag, &tips, None, &self.policy)
            .map_err(|what| Broken::of(28, what))?;
        self.records.push(Record {
            step: n,
            tips,
            at: None,
            reading,
        });
        Ok(())
    }

    /// Every invariant, after a step.
    /// Then the host's own invariants: `host` is called with each actor
    /// and what its replica holds, and its first refusal is the script's.
    pub(super) fn check(
        &mut self,
        host: &mut impl FnMut(Who, &Held<E>) -> Result<(), Broken>,
    ) -> Result<(), Broken> {
        let mut seen = Vec::new();
        let mut actors = Vec::new();
        for who in Who::ALL {
            let kept = &self.replicas[who.index()];
            let findings = kept.verify().map_err(failed(68, "a verify"))?;
            actors.push((who, seen.len()));
            seen.push(self.view(
                who.name().to_owned(),
                kept,
                &self.shadows[who.index()],
                &findings,
            )?);
            if let Some(base) = kept.base() {
                let findings = base
                    .held()
                    .map_err(failed(68, "a base does not read"))?
                    .dag
                    .verify(&Everything);
                seen.push(self.view(
                    format!("{}'s base", who.name()),
                    base,
                    &self.bases[who.index()],
                    &findings,
                )?);
            }
        }
        self.clean(&seen)?;
        self.climb(&seen)?;
        alike(&seen)?;
        self.recorded(&seen)?;
        self.cached(&seen)?;
        for (who, at) in actors {
            let view = &seen[at];
            host(
                who,
                &Held {
                    dag: Arc::clone(&view.dag),
                    folded: Arc::clone(&view.folded),
                    tips: view.tips.clone(),
                },
            )?;
        }
        Ok(())
    }

    /// In an honest script, the union of the replicas verifies clean: a
    /// replica may lack a signature that proves an object it holds, since
    /// a signature rests on what it signs and a partial sync need not take
    /// it, but some replica holds every signature (law 68).
    fn clean(&self, seen: &[Seen<E>]) -> Result<(), Broken> {
        match self.conduct {
            Conduct::Faulty => return Ok(()),
            Conduct::Honest => {}
        }
        let union: BTreeMap<Hash, Envelope<E>> = seen
            .iter()
            .flat_map(|view| view.dag.objects().clone())
            .collect();
        let expected = self.keys.findings(&union);
        if expected.is_empty() {
            Ok(())
        } else {
            Err(Broken::of(
                68,
                format!("the replicas of an honest script together expect findings {expected:?}"),
            ))
        }
    }

    /// The invariants of one replica: its kept fold is the fold of what it
    /// holds (law 45), its tips and heads are its history's (laws 43 and
    /// 65), what it answered it took is what it holds (law 68), and
    /// `verify` finds exactly the script's faults that it holds and the
    /// objects that no key proves there by the script's account of keys
    /// (law 42).
    fn view(
        &self,
        label: String,
        replica: &impl Replica<E>,
        shadow: &Shadow<E>,
        findings: &[Finding],
    ) -> Result<Seen<E>, Broken> {
        let says = |law: u32, what: String| Broken::of(law, format!("{label}: {what}"));
        let held = replica
            .held()
            .map_err(|refused| says(68, format!("does not read: {refused}")))?;
        let nodes = held
            .dag
            .nodes()
            .map_err(|refused| says(43, format!("holds no history: {refused}")))?;
        let fresh = Registers::of(&fold(&nodes));
        if let Some(difference) = Registers::of(&held.folded).differs(&fresh) {
            return Err(says(
                45,
                format!("its kept fold is not the fold: {difference}"),
            ));
        }
        if held.tips != held.dag.tips() {
            return Err(says(43, "its tips are not its history's".to_owned()));
        }
        let heads = replica
            .heads()
            .map_err(|refused| says(65, format!("has no heads: {refused}")))?;
        if heads != held.dag.heads() {
            return Err(says(
                65,
                format!("its heads {heads:?} are not its history's"),
            ));
        }
        let objects: BTreeSet<Hash> = held.dag.objects().keys().cloned().collect();
        if shadow.names != objects {
            return Err(says(
                68,
                crate::laws::reading::sets("what it answered it took", &shadow.names, &objects),
            ));
        }
        if let Some(difference) = Registers::of(&shadow.folded).differs(&fresh) {
            return Err(says(
                68,
                format!("what it answered it took folds otherwise: {difference}"),
            ));
        }
        let mut expected: Vec<Finding> = self
            .faults
            .iter()
            .filter(|(name, _)| objects.contains(*name))
            .map(|(_, finding)| finding.clone())
            .collect();
        expected.extend(self.keys.findings(held.dag.objects()));
        let shown = |findings: &[Finding]| -> BTreeSet<String> {
            findings
                .iter()
                .map(|finding| format!("{finding:?}"))
                .collect()
        };
        let (found, expected) = (shown(findings), shown(&expected));
        if found != expected || findings.len() != found.len() {
            return Err(says(
                42,
                crate::laws::reading::sets("verify finds", &found, &expected),
            ));
        }
        Ok(Seen {
            reading: Reading::of(&held.dag, &held.folded, None, &self.policy),
            tips: held.tips,
            dag: held.dag,
            folded: held.folded,
            label,
        })
    }

    /// No reading of an inflationary register falls as objects arrive: a
    /// replica only gains objects, and each register reads the greatest of
    /// all its writes, so each entity reads at or above what it read at the
    /// last check, under the policy where everything binds (law 32).
    fn climb(&mut self, seen: &[Seen<E>]) -> Result<(), Broken> {
        for view in seen {
            if let Some(then) = self.climbed.get(&view.label) {
                climbed(then, &view.folded)
                    .map_err(|what| Broken::of(32, format!("{}: {what}", view.label)))?;
            }
            self.climbed
                .insert(view.label.clone(), Arc::clone(&view.folded));
        }
        Ok(())
    }

    /// Every reading recorded at a down-set reads the same in every replica
    /// that holds the down-set, however the replica grew (law 68).
    fn recorded(&self, seen: &[Seen<E>]) -> Result<(), Broken> {
        for record in &self.records {
            for view in seen
                .iter()
                .filter(|view| record.tips.iter().all(|tip| view.dag.get(tip).is_some()))
            {
                let now = Reading::at_down_set(&view.dag, &record.tips, record.at, &self.policy)
                    .map_err(|what| Broken::of(43, what))?;
                if let Some(difference) = now.differs(&record.reading) {
                    return Err(Broken::of(
                        68,
                        format!(
                            "{} reads the down-set recorded at step {} otherwise: {difference}",
                            view.label, record.step
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Every reading the cache holds is the fresh reading of every replica
    /// whose frontier its key names (law 51).
    fn cached(&mut self, seen: &[Seen<E>]) -> Result<(), Broken> {
        for (key, kept) in self.memo.pairs() {
            let at = self.instants.get(&key.function).copied().flatten();
            for view in seen
                .iter()
                .filter(|view| frontier(&view.tips) == key.argument)
            {
                let nodes = view.dag.nodes().map_err(failed(43, "a history"))?;
                let fresh = Reading::of(&view.dag, &fold(&nodes), at, &self.policy);
                if let Some(difference) = kept.differs(&fresh) {
                    return Err(Broken::of(
                        51,
                        format!("the cache holds for {}: {difference}", view.label),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Each entity of `then` reads, in `now`, at or above what it read in
/// `then` ([`Product::climbed`]).
fn climbed<E: Schema>(then: &Folded<E>, now: &Folded<E>) -> Result<(), String> {
    for (genesis, prodrome) in then.prodromes() {
        for (key, earlier) in prodrome {
            read(now.stream(genesis, key), None, &Everything)
                .climbed(&read(earlier, None, &Everything))
                .map_err(|fell| format!("entity {key:?}: {fell}"))?;
        }
    }
    Ok(())
}

/// Replicas that hold the same objects read alike: the same tips, and the
/// same registers, claimed and confirmed (laws 33 and 35).
fn alike<E: Schema>(seen: &[Seen<E>]) -> Result<(), Broken> {
    let mut first: BTreeMap<BTreeSet<&Hash>, &Seen<E>> = BTreeMap::new();
    for view in seen {
        let objects: BTreeSet<&Hash> = view.dag.objects().keys().collect();
        let Some(other) = first.get(&objects) else {
            first.insert(objects, view);
            continue;
        };
        if view.tips != other.tips {
            return Err(Broken::of(
                35,
                format!(
                    "{} and {} hold the same objects and not the same tips",
                    view.label, other.label
                ),
            ));
        }
        if let Some(difference) = view.reading.differs(&other.reading) {
            return Err(Broken::of(
                33,
                format!(
                    "{} and {} hold the same objects and read otherwise: {difference}",
                    view.label, other.label
                ),
            ));
        }
    }
    Ok(())
}

/// Whether every step of a script is honest ([`Step::honest`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Conduct {
    Honest,
    Faulty,
}

impl Conduct {
    pub(super) fn of<E>(steps: &[Step<E>]) -> Conduct {
        if steps.iter().all(Step::honest) {
            Conduct::Honest
        } else {
            Conduct::Faulty
        }
    }
}

/// Which shadow an answer is applied to: the replica's, or its overlay's
/// base's.
#[derive(Debug, Clone, Copy)]
enum Layer {
    Replica,
    Base,
}
