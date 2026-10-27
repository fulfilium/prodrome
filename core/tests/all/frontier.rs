//! Law 47 (SPEC §9): a pull that names its replica by a frontier token.
//!
//! A store of record answers two replicas and one other writer, and keeps
//! one, two or three frontiers, so that it lets frontiers go often. The
//! replicas write, post, and pull one piece at a time, and the pieces of
//! one pull interleave with every other step. The store of record can
//! forget every frontier it recorded, and it can go back to what it held
//! after an earlier post. A replica can go back to an earlier state of its
//! own. A model of the record says which tokens the store of record still
//! holds, and each answer reaches what the model says it reaches. The
//! histories are two writers' logs over one shared history, drawn by the
//! generator that every law reads.

use crate::common;

use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::sync::Mutex;

use prodrome::dag::Dag;
use prodrome::event::{canonical_envelope, parents_of, seal_hash, Envelope, Hash, TodoEvent};
use prodrome::genesis::mk_genesis;
use prodrome::reference::Todo;
use prodrome::store::frontier::{Frontiers, Reached, Records, Since, Token, Unrecorded};
use prodrome::store::{sync, MemoryStore, Replica};
use proptest::prelude::*;

use common::two_writers;

type Event = TodoEvent<Todo>;
type Memory = MemoryStore<Event>;
type Prints = Vec<(Hash, Vec<u8>)>;

/// The store whose frontiers the store of record records.
const STORE: &str = "notes";

/// The genesis that every replica of the store begins from.
fn genesis() -> (Hash, Vec<u8>) {
    let genesis: Envelope<Event> =
        Envelope::Genesis(mk_genesis(STORE, &"0".repeat(32)).expect("32 hex digits are a nonce"));
    (
        seal_hash(&genesis),
        canonical_envelope(&genesis).into_bytes(),
    )
}

/// A replica holding `prints` and what they rest on.
fn holding(prints: &Prints) -> Memory {
    let replica = Memory::default();
    let seeds = prints.iter().map(|(name, _)| name.clone()).collect();
    replica
        .receive(seeds, &|name| {
            prints
                .iter()
                .find(|(held, _)| held == name)
                .map(|(_, print)| print.clone())
        })
        .expect("the prints are objects closed under parents");
    replica
}

fn dag(replica: &Memory) -> Dag<Event> {
    (*replica.held().expect("memory reads").dag).clone()
}

fn objects(replica: &Memory) -> BTreeSet<Hash> {
    dag(replica).objects().keys().cloned().collect()
}

/// Every object of `replica` and its print.
fn prints(replica: &Memory) -> Prints {
    objects(replica)
        .into_iter()
        .map(|name| {
            let print = replica.print(&name).expect("a replica holds its prints");
            (name, print)
        })
        .collect()
}

/// The tips of `held`, a set of `dag`'s objects closed under parents.
fn tips(dag: &Dag<Event>, held: &BTreeSet<Hash>) -> BTreeSet<Hash> {
    let below: BTreeSet<Hash> = held
        .iter()
        .filter_map(|name| dag.get(name))
        .flat_map(parents_of)
        .collect();
    held.difference(&below).cloned().collect()
}

/// What the store of record holds of its record, as the law says: each
/// token with its tips, newest last, at most `kept` of them.
struct Model {
    kept: usize,
    ring: Vec<(Token, BTreeSet<Hash>)>,
}

impl Model {
    /// What `since` reaches in `dag`, or `None` when it is unknown.
    fn reached(&self, since: &Since, dag: &Dag<Event>) -> Option<BTreeSet<Hash>> {
        match since {
            Since::Nothing => Some(BTreeSet::new()),
            Since::Frontier(token) => self
                .ring
                .iter()
                .find(|(kept, _)| kept == token)
                .filter(|(_, tips)| tips.iter().all(|tip| dag.get(tip).is_some()))
                .map(|(_, tips)| dag.closure(tips.iter().cloned())),
        }
    }

    fn recorded(&mut self, tips: BTreeSet<Hash>) -> Token {
        let token = Token::of(&tips);
        self.ring.retain(|(kept, _)| kept != &token);
        self.ring.push((token.clone(), tips));
        let gone = self.ring.len().saturating_sub(self.kept);
        self.ring.drain(..gone);
        token
    }
}

/// The store of record: its objects, the frontiers it recorded and their
/// model, and what it held after each post, to go back to.
struct Record {
    store: Memory,
    frontiers: Frontiers,
    model: Model,
    earlier: Vec<Prints>,
}

impl Record {
    fn new(start: &Prints, kept: NonZeroUsize) -> Self {
        Self {
            store: holding(start),
            frontiers: Frontiers::remembered(kept),
            model: Model {
                kept: kept.get(),
                ring: Vec::new(),
            },
            earlier: vec![start.clone()],
        }
    }

    fn forget(&mut self) {
        self.frontiers = Frontiers::remembered(
            NonZeroUsize::new(self.model.kept).expect("the kept number is not zero"),
        );
        self.model.ring.clear();
    }

    /// The answer reaches exactly what the model says, and lacks the rest
    /// in the store's linear order. Answers what it reaches, or `None`.
    fn read(
        &self,
        since: &Since,
        dag: &Dag<Event>,
        reached: &Reached,
        lacking: &[(&Hash, &Envelope<Event>)],
    ) -> Result<Option<BTreeSet<Hash>>, TestCaseError> {
        let expected = self.model.reached(since, dag);
        match (&expected, reached) {
            (Some(model), Reached::Known(answer)) => {
                prop_assert_ne!(
                    model.len(),
                    dag.objects().len(),
                    "a token at the store's tips reaches everything, unwalked"
                );
                prop_assert_eq!(model, answer);
            }
            (Some(model), Reached::Everything) => {
                prop_assert_eq!(
                    model.len(),
                    dag.objects().len(),
                    "everything is every object"
                );
            }
            (None, Reached::Unknown) => {}
            (Some(_), Reached::Unknown) | (None, Reached::Known(_) | Reached::Everything) => {
                return Err(TestCaseError::fail(format!(
                    "the answer is {reached:?}, and the model says {expected:?}"
                )))
            }
        }
        let from = expected.clone().unwrap_or_default();
        let order: Vec<Hash> = dag
            .linearise()
            .expect("a store is closed under parents")
            .into_iter()
            .filter(|name| !from.contains(name))
            .collect();
        let names: Vec<Hash> = lacking.iter().map(|(name, _)| (*name).clone()).collect();
        prop_assert_eq!(names, order);
        Ok(expected)
    }
}

/// A replica, and the frontier of the last answer that it took.
struct Puller {
    replica: Memory,
    since: Since,
    earlier: Vec<Prints>,
}

impl Puller {
    fn new(start: &Prints) -> Self {
        Self {
            replica: holding(start),
            since: Since::Nothing,
            earlier: vec![start.clone()],
        }
    }

    /// An answer that names `token` for the set `held` of the store's
    /// objects, taken: the frontier names only objects the replica holds,
    /// and the replica names it in its next pull.
    fn adopt(
        &mut self,
        record: &mut Record,
        dag: &Dag<Event>,
        held: &BTreeSet<Hash>,
        answered: &prodrome::store::frontier::Recorded,
    ) -> Result<(), TestCaseError> {
        let tips = tips(dag, held);
        prop_assert!(
            tips.is_subset(&objects(&self.replica)),
            "safe: a frontier names only objects the replica holds"
        );
        prop_assert_eq!(answered.count, held.len());
        prop_assert_eq!(&record.model.recorded(tips), &answered.token);
        self.since = Since::Frontier(answered.token.clone());
        self.earlier.push(prints(&self.replica));
        Ok(())
    }

    /// One piece of a pull, of at most `size` objects. Answers the number
    /// of objects still lacking, and whether the piece fell back.
    fn piece(&mut self, record: &mut Record, size: usize) -> Result<(usize, bool), TestCaseError> {
        let dag = dag(&record.store);
        let lacking = record
            .frontiers
            .answer(STORE, &self.since, &dag)
            .expect("memory reads");
        let reached = record.read(&self.since, &dag, lacking.reached(), lacking.lacking())?;
        let sent: Vec<Hash> = lacking
            .lacking()
            .iter()
            .take(size)
            .map(|(name, _)| (*name).clone())
            .collect();
        let rest = lacking.lacking().len() - sent.len();
        let answered = lacking.record(size).expect("memory keeps");
        self.replica
            .receive(sent.iter().cloned().collect(), &|name| {
                record.store.print(name)
            })
            .expect("a piece is closed under parents over what the replica holds");
        let fell_back = reached.is_none();
        let mut held = reached.unwrap_or_default();
        held.extend(sent);
        self.adopt(record, &dag, &held, &answered)?;
        Ok((rest, fell_back))
    }

    /// What the replica holds, posted, and the frontier of the receipt.
    fn post(&mut self, record: &mut Record) -> Result<(), TestCaseError> {
        sync(&self.replica, &record.store).expect("a replica's objects verify");
        record.earlier.push(prints(&record.store));
        let dag = dag(&record.store);
        let lacking = record
            .frontiers
            .answer(STORE, &self.since, &dag)
            .expect("memory reads");
        let reached = record.read(&self.since, &dag, lacking.reached(), lacking.lacking())?;
        let posted = objects(&self.replica);
        let answered = lacking
            .record_with(posted.iter().cloned())
            .expect("memory keeps");
        let mut held = reached.unwrap_or_default();
        held.extend(dag.closure(posted));
        self.adopt(record, &dag, &held, &answered)
    }

    /// The replica's earlier state: it holds less than its frontier, so it
    /// names nothing.
    fn go_back(&mut self, to: &prop::sample::Index) {
        self.replica = holding(to.get(&self.earlier));
        self.since = Since::Nothing;
    }

    /// Pieces until the pull ends, with nothing else between them: at most
    /// one falls back, and the pull ends.
    fn pull(&mut self, record: &mut Record, size: usize) -> Result<(), TestCaseError> {
        let bound = objects(&record.store).len() / size + 2;
        let mut fell_back = 0;
        for _ in 0..bound {
            let (rest, fell) = self.piece(record, size)?;
            fell_back += usize::from(fell);
            prop_assert!(fell_back <= 1, "a pull falls back once");
            if rest == 0 {
                return Ok(());
            }
        }
        Err(TestCaseError::fail("a pull ends"))
    }
}

#[derive(Debug, Clone)]
enum Step {
    /// The other writer appends its next event and posts.
    Other,
    /// A replica appends its next event, and does not post.
    Write(usize),
    /// A replica posts what it holds.
    Post(usize),
    /// A replica asks for one piece of `size` objects.
    Piece(usize, usize),
    /// The store of record forgets every frontier it recorded.
    Forget,
    /// The store of record goes back to what it held after an earlier
    /// post, and keeps its records.
    GoBack(prop::sample::Index),
    /// A replica goes back to an earlier state of its own.
    Lose(usize, prop::sample::Index),
}

fn a_step() -> impl Strategy<Value = Step> {
    let who = 0..2usize;
    prop_oneof![
        3 => Just(Step::Other),
        2 => who.clone().prop_map(Step::Write),
        1 => who.clone().prop_map(Step::Post),
        5 => (who.clone(), 1..5usize).prop_map(|(who, size)| Step::Piece(who, size)),
        1 => Just(Step::Forget),
        1 => any::<prop::sample::Index>().prop_map(Step::GoBack),
        1 => (who, any::<prop::sample::Index>()).prop_map(|(who, to)| Step::Lose(who, to)),
    ]
}

/// The next event of `log` appended to `replica`. A write that the
/// replica refuses, for example one that revises a todo it does not hold,
/// writes nothing, and the law holds over whatever the replica holds.
fn write(replica: &Memory, log: &mut std::vec::IntoIter<Event>) {
    if let Some(event) = log.next() {
        let _refused = replica.append(event);
    }
}

proptest! {
    #![proptest_config(common::cases::cases(96))]

    /// Law 47: a pull by a frontier token is the pull from its tips. Each
    /// answer reaches exactly the closure of its token's tips when the
    /// model of the record holds the token and the store holds the tips,
    /// and nothing otherwise, and it lacks the rest in the store's linear
    /// order. Each frontier names only objects the replica holds (safe). A
    /// pull with nothing between its pieces falls back at most once and
    /// ends, leaving the replica holding every object of the store
    /// (complete). A pull from a known frontier that reaches every object
    /// sends nothing (quiet), and records the same token.
    #[test]
    fn a_pull_by_frontier_token_is_the_pull_from_its_tips(
        (shared, mine, theirs) in two_writers(),
        steps in prop::collection::vec(a_step(), 1..60),
        kept in 1..4usize,
        size in 1..6usize,
    ) {
        let begun = holding(&vec![genesis()]);
        for event in shared {
            begun.append(event).expect("the shared history appends");
        }
        let start = prints(&begun);
        let kept = NonZeroUsize::new(kept).expect("the strategy draws no zero");
        let mut record = Record::new(&start, kept);
        let mut other = Puller::new(&start);
        let mut pullers = [Puller::new(&start), Puller::new(&start)];
        let (mut mine, mut theirs) = (mine.into_iter(), theirs.into_iter());
        for step in &steps {
            match step {
                Step::Other => {
                    write(&other.replica, &mut theirs);
                    other.post(&mut record)?;
                }
                Step::Write(who) => write(&pullers[*who].replica, &mut mine),
                Step::Post(who) => pullers[*who].post(&mut record)?,
                Step::Piece(who, size) => {
                    pullers[*who].piece(&mut record, *size)?;
                }
                Step::Forget => record.forget(),
                Step::GoBack(to) => record.store = holding(to.get(&record.earlier)),
                Step::Lose(who, to) => pullers[*who].go_back(to),
            }
        }
        for puller in &mut pullers {
            puller.pull(&mut record, size)?;
            prop_assert!(
                objects(&record.store).is_subset(&objects(&puller.replica)),
                "complete"
            );
        }
        for puller in &mut pullers {
            let before = puller.since.clone();
            let (rest, fell_back) = puller.piece(&mut record, size)?;
            prop_assert_eq!(rest, 0, "quiet");
            prop_assert!(!fell_back, "quiet");
            prop_assert_eq!(&puller.since, &before, "quiet");
        }
    }
}

/// Records kept in memory that fail at one write, to stand for a host
/// that stops between two of the writes of one record.
struct Failing {
    sets: Mutex<Vec<(Token, BTreeSet<Hash>)>>,
    ring: Mutex<Vec<Token>>,
    writes: Mutex<usize>,
    fails_at: usize,
}

impl Failing {
    fn write(&self) -> Result<(), Unrecorded> {
        let mut writes = self.writes.lock().expect("no test panics while it is held");
        *writes += 1;
        if *writes == self.fails_at {
            Err(Unrecorded("the host stopped".to_owned()))
        } else {
            Ok(())
        }
    }
}

impl Records for &'static Failing {
    fn tips(&self, _: &str, token: &Token) -> Result<Option<BTreeSet<Hash>>, Unrecorded> {
        Ok(self
            .sets
            .lock()
            .expect("no test panics while it is held")
            .iter()
            .find(|(kept, _)| kept == token)
            .map(|(_, tips)| tips.clone()))
    }

    fn keep(&self, _: &str, token: &Token, tips: &BTreeSet<Hash>) -> Result<(), Unrecorded> {
        self.write()?;
        let mut sets = self.sets.lock().expect("no test panics while it is held");
        sets.retain(|(kept, _)| kept != token);
        sets.push((token.clone(), tips.clone()));
        Ok(())
    }

    fn let_go(&self, _: &str, token: &Token) -> Result<(), Unrecorded> {
        self.write()?;
        self.sets
            .lock()
            .expect("no test panics while it is held")
            .retain(|(kept, _)| kept != token);
        Ok(())
    }

    fn ring(&self, _: &str) -> Result<Vec<Token>, Unrecorded> {
        Ok(self
            .ring
            .lock()
            .expect("no test panics while it is held")
            .clone())
    }

    fn keep_ring(&self, _: &str, ring: &[Token]) -> Result<(), Unrecorded> {
        self.write()?;
        *self.ring.lock().expect("no test panics while it is held") = ring.to_vec();
        Ok(())
    }
}

proptest! {
    #![proptest_config(common::cases::cases(64))]

    /// Law 47: a host that stops at any write of a record leaves records
    /// that only read as unknown or as what was recorded. Every kept set
    /// of tips is listed in the ring, the ring keeps at most the kept
    /// number, and each token that reads tips reads the tips it names. A
    /// pull from nothing afterwards still ends with everything.
    #[test]
    fn a_record_stopped_at_any_write_reads_as_unknown_or_as_recorded(
        (shared, _, _) in two_writers(),
        fails_at in 1..40usize,
        kept in 1..3usize,
    ) {
        let begun = holding(&vec![genesis()]);
        for event in shared {
            begun.append(event).expect("the shared history appends");
        }
        let failing: &'static Failing = Box::leak(Box::new(Failing {
            sets: Mutex::default(),
            ring: Mutex::default(),
            writes: Mutex::new(0),
            fails_at,
        }));
        let kept = NonZeroUsize::new(kept).expect("the strategy draws no zero");
        let frontiers = Frontiers::over(failing, kept);
        let dag = dag(&begun);
        let order = dag.linearise().expect("a store is closed under parents");
        let mut since = Since::Nothing;
        for _ in 0..=order.len() {
            let lacking = frontiers.answer(STORE, &since, &dag).expect("memory reads");
            match lacking.record(1) {
                Ok(answered) => since = Since::Frontier(answered.token),
                Err(_) => since = Since::Nothing,
            }
            let ring = failing.ring(STORE).expect("memory reads");
            prop_assert!(ring.len() <= kept.get());
            let sets = failing.sets.lock().expect("no test panics while it is held");
            for (token, tips) in sets.iter() {
                prop_assert!(ring.contains(token), "no tips are kept that no ring lists");
                prop_assert_eq!(token, &Token::of(tips));
            }
        }
        // One write fails in all, so a second try of the whole pull is answered.
        for _ in 0..2 {
            let lacking = frontiers.answer(STORE, &since, &dag).expect("memory reads");
            if let Ok(answered) = lacking.record(usize::MAX) {
                since = Since::Frontier(answered.token);
                break;
            }
        }
        let lacking = frontiers.answer(STORE, &since, &dag).expect("memory reads");
        prop_assert_eq!(lacking.reached(), &Reached::Everything);
        prop_assert!(lacking.lacking().is_empty());
        prop_assert_eq!(lacking.record(1).expect("memory keeps").count, order.len());
    }
}

/// A writer that posts one new todo each time.
struct Writer {
    puller: Puller,
    written: usize,
}

impl Writer {
    fn post(&mut self, record: &mut Record) -> Result<(), TestCaseError> {
        let todo = format!("t{}", self.written);
        let at = common::moment(i64::try_from(self.written).expect("a small count"));
        self.written += 1;
        let event = prodrome::event::mk_created(&todo, at, "ada", "", "").expect("a todo");
        self.puller
            .replica
            .append(event)
            .expect("a new todo appends");
        self.puller.post(record)
    }
}

/// A store of record keeping `kept` frontiers, holding the genesis and
/// `first` objects that a writer posted, and a replica that holds nothing.
fn begun(kept: usize, first: usize) -> Result<(Record, Writer, Puller), TestCaseError> {
    let start = vec![genesis()];
    let kept = NonZeroUsize::new(kept).expect("a kept number is not zero");
    let mut record = Record::new(&start, kept);
    let mut writer = Writer {
        puller: Puller::new(&start),
        written: 0,
    };
    for _ in 0..first {
        writer.post(&mut record)?;
    }
    Ok((record, writer, Puller::new(&start)))
}

/// Law 47 (c): with a kept number of 3, a writer that posts 3 times
/// between each two pieces of a pull lets the pull's token go each time.
/// Every piece after the first falls back and sends the same first
/// objects, so the pull does not end, although each piece sends more
/// objects than the store gains.
#[test]
fn a_pull_whose_token_is_let_go_between_its_pieces_does_not_end() -> Result<(), TestCaseError> {
    let (mut record, mut writer, mut slow) = begun(3, 5)?;
    for round in 0..40 {
        let (rest, fell_back) = slow.piece(&mut record, 4)?;
        prop_assert!(rest > 0, "the pull has not ended in round {}", round);
        prop_assert_eq!(fell_back, round > 0, "round {}", round);
        for _ in 0..3 {
            writer.post(&mut record)?;
        }
    }
    Ok(())
}

/// Law 47 (c): with pieces of one object, a writer that posts one object
/// between each two pieces keeps the number of lacking objects the same.
/// No piece falls back, and the pull does not end.
#[test]
fn a_pull_whose_store_gains_a_piece_between_its_pieces_does_not_end() -> Result<(), TestCaseError> {
    let (mut record, mut writer, mut slow) = begun(3, 5)?;
    let (first, _) = slow.piece(&mut record, 1)?;
    for round in 0..40 {
        writer.post(&mut record)?;
        let (rest, fell_back) = slow.piece(&mut record, 1)?;
        prop_assert!(!fell_back, "round {}", round);
        prop_assert_eq!(rest, first, "round {}", round);
    }
    Ok(())
}

/// Law 47 (a): a token reads as unknown when the tips its records hold
/// are not the tips it names, so a torn or misplaced record never widens
/// what a token reaches.
#[test]
fn a_record_whose_tips_are_not_its_token_s_reads_as_unknown() {
    let failing: &'static Failing = Box::leak(Box::new(Failing {
        sets: Mutex::default(),
        ring: Mutex::default(),
        writes: Mutex::new(0),
        fails_at: usize::MAX,
    }));
    let frontiers = Frontiers::over(failing, NonZeroUsize::MIN);
    let dag = dag(&holding(&vec![genesis()]));
    let token = frontiers
        .answer(STORE, &Since::Nothing, &dag)
        .expect("memory reads")
        .record(1)
        .expect("memory keeps")
        .token;
    let since = Since::Frontier(token);
    let reached = |frontiers: &Frontiers| {
        frontiers
            .answer(STORE, &since, &dag)
            .expect("memory reads")
            .reached()
            .clone()
    };
    assert_eq!(reached(&frontiers), Reached::Everything);
    for (_, tips) in failing
        .sets
        .lock()
        .expect("no test panics while it is held")
        .iter_mut()
    {
        tips.clear();
    }
    assert_eq!(reached(&frontiers), Reached::Unknown);
}

/// Law 47 (d): a pull from a known token whose tips are the store's tips
/// reaches everything, lacks nothing, and records the same token with the
/// count of every object; over a DAG with one more object, the same token
/// reaches only what it names.
#[test]
fn a_token_at_the_store_s_tips_reaches_everything_and_lacks_nothing() {
    use proptest::strategy::ValueTree as _;
    let store = holding(&vec![genesis()]);
    let mut puller = Puller::new(&vec![genesis()]);
    let mut record = Record::new(&vec![genesis()], prodrome::store::frontier::KEPT);
    let (shared, _, _) = two_writers()
        .new_tree(&mut proptest::test_runner::TestRunner::deterministic())
        .expect("a history")
        .current();
    for event in shared {
        store.append(event).expect("the shared history appends");
    }
    record.store = store;
    puller.pull(&mut record, usize::MAX).expect("the pull ends");
    let dag = dag(&record.store);
    let Since::Frontier(token) = puller.since.clone() else {
        panic!("a pull names a token");
    };
    let lacking = record
        .frontiers
        .answer(STORE, &puller.since, &dag)
        .expect("memory reads");
    assert_eq!(lacking.reached(), &Reached::Everything);
    assert!(lacking.lacking().is_empty());
    let recorded = lacking.record(usize::MAX).expect("memory keeps");
    assert_eq!(recorded.token, token);
    assert_eq!(recorded.count, dag.objects().len());

    // One object more than the token's tips: the same token reaches what
    // it names, and lacks the new object.
    let more = holding(&prints(&record.store));
    let mut log = two_writers()
        .new_tree(&mut proptest::test_runner::TestRunner::deterministic())
        .expect("a history")
        .current()
        .1
        .into_iter();
    while objects(&more).len() == dag.objects().len() {
        write(&more, &mut log);
    }
    let grown = self::dag(&more);
    let lacking = record
        .frontiers
        .answer(STORE, &Since::Frontier(token), &grown)
        .expect("memory reads");
    assert_eq!(
        lacking.reached(),
        &Reached::Known(dag.objects().keys().cloned().collect())
    );
    assert_eq!(
        lacking.lacking().len(),
        grown.objects().len() - dag.objects().len()
    );
}
