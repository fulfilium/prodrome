//! Law 68, the script law (`prodrome::laws::script`), over the todo schema.
//!
//! Each actor writes every kind of the todo schema, under its own name, at
//! one of four instants an hour apart, so that writes collide in one tick
//! and arrive after readings at later instants. Specs reference other todos
//! and `Absent`, so the writes link.

use prodrome::event::{
    canonical_envelope, mk_cancelled, mk_completed, mk_created, mk_reopened, mk_tended, seal_hash,
    Envelope, TodoEvent, TodoId,
};
use prodrome::fold::read;
use prodrome::fpl::{instant_of, Candidates, Outcome};
use prodrome::genesis::mk_genesis;
use prodrome::laws::script::{
    self, a_script, an_honest_script, Kind, Root, Script, Step, Through, Who, Writing,
};
use prodrome::literal::Datetime;
use prodrome::policy::Everything;
use prodrome::reference::Todo;
use prodrome::store::{sync, MemoryStore, Replica};
use prodrome::tags::alias::{self, AliasVocabulary};
use prodrome::tags::membership::{self, MembershipVocabulary};
use prodrome::todo::TodoVocabulary;
use proptest::prelude::*;

use crate::common::{a_draft_with, a_spec_with_absence, moment, Draft};
use crate::schema_laws::failed;
use crate::tags::{self, label, note, Aliases, Label, Memberships, LABELS};

type Event = TodoEvent<Todo>;

/// The longest script the normal run draws.
const STEPS: usize = 40;

/// A todo event that `who` writes, at one of four instants an hour apart.
fn a_todo_write(who: Who) -> impl Strategy<Value = Event> {
    (a_draft_with(a_spec_with_absence()), 0..4i64).prop_map(move |(draft, hour)| {
        Draft {
            actor: who.name(),
            ..draft
        }
        .at(moment(hour * 3600))
    })
}

fn runs(script: &Script<Event>) -> Result<script::Ran, TestCaseError> {
    script::run(&TodoVocabulary::default(), script).map_err(failed)
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Law 68: every invariant holds after every step of a script of todo
    /// writes with faults among them.
    #[test]
    fn every_step_of_a_todo_script_keeps_every_invariant(script in a_script(a_todo_write, STEPS)) {
        runs(&script)?;
    }

    /// Law 68: after every step of an honest script, `verify` finds nothing.
    #[test]
    fn an_honest_todo_script_verifies_clean(script in an_honest_script(a_todo_write, STEPS)) {
        runs(&script)?;
    }
}

/// Law 68 over many more scripts, for the nightly run: `cargo test -p
/// prodrome-core --test all script::a_long_run -- --ignored`. It prints the
/// rate ([`script::long_run`]).
#[test]
#[ignore = "the nightly run calls it"]
fn a_long_run_of_todo_scripts_keeps_every_invariant() {
    match script::long_run(
        &a_script(a_todo_write, 2 * STEPS),
        crate::common::cases::cases(1024),
        |script| script::run(&TodoVocabulary::default(), script),
    ) {
        Ok(rate) => println!("{rate}"),
        Err(failure) => panic!("{failure}"),
    }
}

/// A tag put on or taken off a note by `who`, at one of four minutes.
fn a_membership_write(who: Who) -> impl Strategy<Value = Memberships> {
    (0..3usize, 0..LABELS.len(), any::<bool>(), 0..4u32).prop_map(move |(n, tag, on, minute)| {
        let (note, tag, at) = (note(n), label(LABELS[tag]), tags::at(minute));
        if on {
            membership::added(note, tag, at, who.actor())
        } else {
            membership::removed(note, tag, at, who.actor())
        }
    })
}

/// A label made an alias of another, or a name of its own again, by `who`.
fn an_alias_write(who: Who) -> impl Strategy<Value = Aliases> {
    (0..LABELS.len(), 1..LABELS.len(), any::<bool>(), 0..4u32).prop_map(
        move |(tag, step, to, minute)| {
            let (tag, at) = (label(LABELS[tag]), tags::at(minute));
            let other = label(LABELS[(tag_index(&tag) + step) % LABELS.len()]);
            if to {
                alias::aliased(tag, other, at, who.actor()).expect("a label and another")
            } else {
                alias::unaliased(tag, at, who.actor()).expect("never an alias of itself")
            }
        },
    )
}

fn tag_index(tag: &Label) -> usize {
    LABELS
        .iter()
        .position(|name| label(name) == *tag)
        .expect("a label of LABELS")
}

proptest! {
    #![proptest_config(crate::common::cases::cases(32))]

    /// Law 68 over the tag set's membership schema (law 48).
    #[test]
    fn every_step_of_a_membership_script_keeps_every_invariant(
        script in a_script(a_membership_write, STEPS),
    ) {
        script::run(&MembershipVocabulary::default(), &script).map_err(failed)?;
    }

    /// Law 68 over the tag set's alias schema (law 48).
    #[test]
    fn every_step_of_an_alias_script_keeps_every_invariant(
        script in a_script(an_alias_write, STEPS),
    ) {
        script::run(&AliasVocabulary::default(), &script).map_err(failed)?;
    }
}

/// An hour after the first instant of the todo writes.
fn hour(n: i64) -> Datetime {
    moment(n * 3600)
}

/// A store in memory that holds the genesis of `of`, as every replica of
/// one prodrome does.
fn begun(of: &MemoryStore<Event>) -> MemoryStore<Event> {
    let store = MemoryStore::default();
    sync(of, &store).expect("syncs");
    store
}

fn first() -> MemoryStore<Event> {
    let genesis: Envelope<Event> =
        Envelope::Genesis(mk_genesis("case", &"cd".repeat(16)).expect("a genesis"));
    let print = canonical_envelope(&genesis).into_bytes();
    let store = MemoryStore::default();
    store
        .receive([seal_hash(&genesis)].into(), &|_| Some(print.clone()))
        .expect("begins");
    store
}

/// The candidate outcomes of `todo` in `store` at `at`.
fn outcomes(store: &MemoryStore<Event>, todo: &str, at: Datetime) -> Candidates {
    let held = store.held().expect("memory reads");
    let todo = TodoId::new(todo).expect("a todo id");
    let outcomes = held
        .folded
        .entities()
        .find(|(key, _)| **key == todo)
        .map_or_else(
            || [None].into(),
            |(_, stream)| read(stream, Some(instant_of(at)), &Everything).outcomes(),
        );
    outcomes
}

fn completed(at: Datetime) -> Candidates {
    [Some(Outcome::Completed(instant_of(at)))].into()
}

/// The first question of the brief, as a script and as readings. A change
/// dated after the instant of a reading is not in that reading, and a
/// reader at a later instant sees it (law 2). A change that arrives late
/// and is dated at or before the instant changes the reading at that
/// instant (SPEC §6: a write joins when it is dated at or before the
/// instant, whenever it arrived). A reading recorded at a down-set does not
/// change (law 68).
#[test]
fn a_late_change_moves_a_reading_at_an_instant_and_not_one_at_a_down_set() {
    let created = mk_created("alpha", hour(0), "ada", "t", "").expect("an event");
    let cancelled = mk_cancelled("alpha", hour(1), "cai", "").expect("an event");
    let done = mk_completed("alpha", hour(3), "ada", "").expect("an event");
    let script = Script {
        kinds: [Kind::Memory, Kind::Disk, Kind::Overlay],
        steps: vec![
            append(Who::Ada, created.clone()),
            append(Who::Cai, cancelled.clone()),
            Step::Sync {
                from: Who::Ada,
                to: Who::Ben,
            },
            Step::Record {
                by: Who::Ben,
                at: 1,
            },
            Step::Read {
                by: Who::Ben,
                at: 1,
                through: Through::Memo,
            },
            append(Who::Ada, done.clone()),
            Step::Sync {
                from: Who::Ada,
                to: Who::Ben,
            },
            Step::Sync {
                from: Who::Cai,
                to: Who::Ben,
            },
            Step::Read {
                by: Who::Ben,
                at: 1,
                through: Through::Memo,
            },
            Step::SyncAll,
        ],
    };
    runs(&script).expect("the script keeps every invariant");

    let ada = first();
    let (ben, cai) = (begun(&ada), begun(&ada));
    ada.append(created).expect("appends");
    cai.append(cancelled).expect("appends");
    sync(&ada, &ben).expect("syncs");
    assert_eq!(
        outcomes(&ben, "alpha", hour(2)),
        [None].into(),
        "open at hour 2"
    );
    ada.append(done).expect("appends");
    sync(&ada, &ben).expect("syncs");
    assert_eq!(
        outcomes(&ben, "alpha", hour(2)),
        [None].into(),
        "a later change is not seen at hour 2"
    );
    assert_eq!(
        outcomes(&ben, "alpha", hour(3)),
        completed(hour(3)),
        "and is seen at hour 3"
    );
    sync(&cai, &ben).expect("syncs");
    assert_eq!(
        outcomes(&ben, "alpha", hour(2)),
        [Some(Outcome::Cancelled(instant_of(hour(1))))].into(),
        "a change dated before hour 2, arriving late, moves the reading at hour 2"
    );
}

/// The second question of the brief: an add, a remove and an add again in
/// one tick. The second add is the first add's event, so the append writes
/// nothing and answers the first add (law 20, SPEC §3), and the todo stays
/// open. A second replica that held what the first add rested on writes the
/// same object for it, so the merge stays open too. A replica that held
/// something else of the todo writes a twin, which the removal did not see,
/// so the merge is a conflict of the add and the removal.
#[test]
fn an_add_a_remove_and_an_add_in_one_tick() {
    let created = mk_created("alpha", hour(0), "ada", "t", "").expect("an event");
    let add = mk_completed("alpha", hour(1), "ada", "").expect("an event");
    let remove = mk_reopened("alpha", hour(1), "ada", "").expect("an event");
    let tended = mk_tended("alpha", hour(0), "cai", "").expect("an event");
    let script = Script {
        kinds: [Kind::Memory, Kind::Overlay, Kind::Disk],
        steps: vec![
            append(Who::Ada, created.clone()),
            Step::SyncAll,
            append(Who::Cai, tended.clone()),
            append(Who::Ada, add.clone()),
            append(Who::Ada, remove.clone()),
            append(Who::Ada, add.clone()),
            Step::Append {
                by: Who::Ben,
                of: Who::Ada,
                event: add.clone(),
                writing: Writing::Unsigned,
            },
            Step::Append {
                by: Who::Cai,
                of: Who::Ada,
                event: add.clone(),
                writing: Writing::Unsigned,
            },
            Step::SyncAll,
        ],
    };
    runs(&script).expect("the script keeps every invariant");

    let ada = first();
    ada.append(created).expect("appends");
    let (ben, cai) = (begun(&ada), begun(&ada));
    cai.append(tended).expect("appends");
    let first_add = ada.append(add.clone()).expect("appends");
    ada.append(remove).expect("appends");
    let held = ada.held().expect("reads").dag.objects().len();
    assert_eq!(
        ada.append(add.clone()).expect("appends"),
        first_add,
        "the add again is the first add"
    );
    assert_eq!(
        ada.held().expect("reads").dag.objects().len(),
        held,
        "and writes nothing"
    );
    assert_eq!(
        outcomes(&ada, "alpha", hour(1)),
        [None].into(),
        "the todo stays open"
    );

    assert_eq!(
        ben.append(add.clone()).expect("appends"),
        first_add,
        "the same heads, the same object"
    );
    sync(&ada, &ben).expect("syncs");
    assert_eq!(
        outcomes(&ben, "alpha", hour(1)),
        [None].into(),
        "the merge stays open"
    );

    assert_ne!(
        cai.append(add).expect("appends"),
        first_add,
        "other heads, a twin"
    );
    sync(&ada, &cai).expect("syncs");
    assert_eq!(
        outcomes(&cai, "alpha", hour(1)),
        [None, Some(Outcome::Completed(instant_of(hour(1))))].into(),
        "the twin is a candidate beside the removal"
    );
}

/// `by` appends `event`, its own, signed.
fn append(by: Who, event: Event) -> Step<Event> {
    Step::Append {
        by,
        of: by,
        event,
        writing: Writing::Signed,
    }
}

/// The shortest script the machine found in which a replica holds a signed
/// write without its signature (law 68): a partial sync takes what a chosen
/// object rests on, and a signature rests on the object it signs, not the
/// other way round. `verify` reports the write as unsigned on that replica
/// until the signature arrives, and the script's account of keys expects
/// exactly that.
#[test]
fn a_partial_sync_can_take_a_signed_write_without_its_signature() {
    let created = mk_created("alpha", hour(0), "cai", "t", "").expect("an event");
    let done = mk_completed("beta", hour(1), "cai", "").expect("an event");
    let mut steps = vec![
        Step::Rotate { by: Who::Cai },
        append(Who::Cai, created),
        append(Who::Cai, done),
    ];
    steps.extend((0..7).map(|at| Step::Pick {
        from: Who::Cai,
        to: Who::Ben,
        chosen: vec![at],
    }));
    steps.push(Step::SyncAll);
    let script = Script {
        kinds: [Kind::Memory, Kind::Memory, Kind::Memory],
        steps,
    };
    runs(&script).expect("the script keeps every invariant");
}

/// The shortest script the machine found that tells "beneath every
/// revocation" from "beneath one revocation" (law 42): ben signs a write,
/// revokes the key standing behind it, and cai revokes the same key
/// without it. After the sync, the write is unsigned everywhere.
#[test]
fn a_key_revoked_twice_keeps_only_what_every_revocation_stands_behind() {
    let created = mk_created("alpha", hour(0), "ben", "t0", "").expect("an event");
    let script = Script {
        kinds: [Kind::Memory, Kind::Memory, Kind::Memory],
        steps: vec![
            Step::Rotate { by: Who::Ben },
            Step::Sync {
                from: Who::Ben,
                to: Who::Cai,
            },
            append(Who::Ben, created),
            Step::Revoke {
                by: Who::Ben,
                of: Who::Ben,
                key: 0,
                root: Root::First,
            },
            Step::Revoke {
                by: Who::Cai,
                of: Who::Ben,
                key: 0,
                root: Root::Second,
            },
            Step::SyncAll,
        ],
    };
    runs(&script).expect("the script keeps every invariant");
}
