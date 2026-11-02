//! The same stored objects as `tests/all/literals.rs`, but through the CLOSED
//! vocabulary of §4 and as TYPED events: parsed into an `Envelope`, printed
//! back, and named by `seal_hash`.
//!
//! `tests/all/literals.rs` proves the grammar round-trips; this proves the KINDS
//! do — that every field the spec declares is read, validated by its `mk_*`,
//! and printed back in declared order. A shipped field this side forgot would
//! show up here as a print that is one field short of its name.

use crate::common;

use std::collections::BTreeSet;

use prodrome::change::mk_change;
use prodrome::event::{
    canonical, canonical_envelope, event_id, parents_of, parse_envelope, seal_hash, Actor,
    Envelope, Hash, TodoEvent, ENVELOPE_SIGNATURES,
};
use prodrome::genesis::mk_genesis;
use prodrome::payload::Payload;
use prodrome::policy::{Policy, Untrusted};
use prodrome::reference::Todo;
use prodrome::snapshot::mk_snapshot;
use prodrome::term::schema::signatures;
use prodrome::todo::TODO_SIGNATURES;
use proptest::prelude::*;

fn roster() -> Untrusted {
    // The roster is the DEPLOYMENT's, never the engine's (§5): it arrives as a
    // parameter, and this is one — the reference policy, which is what the
    // corpus was written under.
    Untrusted::of([Actor::new("triage").expect("valid")])
}

#[test]
fn every_stored_object_is_a_typed_envelope_that_prints_back() {
    let corpus = common::corpus();
    assert!(corpus.len() > 5, "a corpus, not a single object");
    for (name, envelope) in &corpus {
        let text = canonical_envelope(envelope);
        let parsed = parse_envelope::<TodoEvent<Todo>>(&Default::default(), &text)
            .unwrap_or_else(|e| panic!("object {}: {e}", name.as_str()));
        assert_eq!(&parsed, envelope, "object {}", name.as_str());
        assert_eq!(
            canonical_envelope(&parsed),
            text,
            "object {}",
            name.as_str()
        );
        assert_eq!(
            seal_hash(&parsed).as_str(),
            name.as_str(),
            "seal_hash names the object"
        );
    }
}

/// §3's envelope shapes, over the corpus: exactly one object rests on nothing,
/// a `Sealed` has one parent, and a `Woven` has two or more and may carry no
/// event at all — a merge is structure.
#[test]
fn the_corpus_holds_every_envelope_shape() {
    let (mut genesis, mut sealed, mut merges) = (0, 0, 0);
    for (name, envelope) in common::corpus() {
        let parents = parents_of(&envelope).len();
        match &envelope {
            Envelope::Sealed { .. } => {
                assert!(parents <= 1, "object {} is a Sealed", name.as_str());
                if parents == 0 {
                    genesis += 1;
                } else {
                    sealed += 1;
                }
            }
            Envelope::Woven { .. } => {
                assert!(parents >= 2, "a Woven names at least two parents");
                if envelope.event().is_none() {
                    merges += 1;
                }
            }
            other => panic!("the legacy corpus holds a {}", other.name()),
        }
    }
    assert_eq!(genesis, 1, "exactly one object rests on nothing");
    assert!(sealed > 1 && merges > 0);
}

/// Every kind the corpus uses is a kind SPEC §4 declares, the reference policy
/// (§5) has a standing for each one, and every spec a stored event carries
/// EVALUATES — not just
/// prints back, which is what the closed seam between §4 and §7 buys.
#[test]
fn the_corpus_uses_the_closed_vocabulary_and_every_spec_it_carries_evaluates() {
    let policy = roster();
    let mut kinds: BTreeSet<&str> = BTreeSet::new();
    let (mut provisional, mut priced) = (0, 0);
    for (name, envelope) in common::corpus() {
        let Some(event) = envelope.event() else {
            continue;
        };
        kinds.insert(event.kind_name());
        if policy.standing(&name, event).claims() {
            provisional += 1;
        }
        if let TodoEvent::Authored(authored) = event {
            // A content record binds whoever wrote it (§5) — and is still shown
            // as its writer's, which is what `confirms` is for.
            assert!(policy.standing(&name, event).binds());
            if let Some(spec) = &authored.payload.spec {
                let now = prodrome::fpl::instant_of(authored.at);
                let spec = prodrome::fpl::Closed::of(spec.clone()).expect("no Ref");
                let value = prodrome::fpl::fulfillment(&spec, now, &prodrome::fpl::Env::new())
                    .expect("the corpus's specs hold no Absent");
                assert!((0.0..=1.0).contains(&value), "todo {:?}", authored.todo);
                priced += 1;
            }
        }
    }
    for kind in &kinds {
        assert!(
            ENVELOPE_SIGNATURES
                .iter()
                .chain(TODO_SIGNATURES)
                .any(|(name, _)| name == kind)
                || *kind == Todo::KIND,
            "{kind} is outside SPEC §4"
        );
    }
    // Every kind §4 declares an EVENT for is exercised; the payload's own
    // records (`Source`, `Note`, `SubTodo`) and the envelopes ride inside them.
    for kind in [
        "Created",
        "Completed",
        "Cancelled",
        "Reopened",
        "Tended",
        "SpecRevised",
        "Authored",
    ] {
        assert!(kinds.contains(kind), "the corpus is missing a {kind}");
    }
    assert!(provisional > 0, "the corpus exercises the standing rule");
    assert!(
        priced > 0,
        "the corpus holds specs, and every one evaluates"
    );
}

/// §4 is a CLOSED vocabulary, so a name outside it is a refusal at the read
/// boundary — where `literal::Open` would have taken the same text.
#[test]
fn a_name_outside_spec_4_is_refused_at_the_envelope() {
    for text in [
        "Sealed(prev='', event=Invented(todo='t', at=datetime(2026, 1, 1, 0, 0, 0)))",
        "Created(todo='t', at=datetime(2026, 1, 1, 0, 0, 0), actor='bassel', text='x', note='')",
        "Sealed(prev='', event=Completed(todo='t', at=datetime(2026, 1, 1, 0, 0, 0)))",
        "Sealed(prev='not a hash', event=None)",
        "Woven(parents=('a',), event=None)",
        "None",
        "Genesis(label='x', nonce='abc')",
        "Genesis(label='x')",
        "Change(genesis='', deps=(), event=None)",
        "Snapshot(genesis='', tips=(), previous='')",
        "Signed(object='', key='', signature='')",
        "KeyAdded(genesis='', actor='bassel', key='')",
        "KeyRevoked(genesis='', deps=(), actor='Bassel', key='')",
    ] {
        assert!(
            parse_envelope::<TodoEvent<Todo>>(&Default::default(), text).is_err(),
            "must refuse {text:?}"
        );
    }
}

/// §2: "The names admitted are the closed vocabulary of §4 and §7 plus
/// `datetime`/`timedelta`." This is the pin on that set — the core's half
/// UNION the payload's, which is what a stored object is read against.
#[test]
fn the_vocabulary_is_exactly_the_spec_s() {
    let mut names: Vec<&str> = signatures()
        .names()
        .chain(
            ENVELOPE_SIGNATURES
                .iter()
                .chain(TODO_SIGNATURES)
                .chain(Todo::VOCABULARY.iter())
                .map(|(name, _)| *name),
        )
        .chain(std::iter::once(Todo::KIND))
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "Absent",
            "After",
            "Authored",
            "Cancelled",
            "Change",
            "Completed",
            "Conj",
            "Created",
            "Curve",
            "CurvePoint",
            "Decay",
            "Flat",
            "Gate",
            "Genesis",
            "Importance",
            "KeyAdded",
            "KeyRevoked",
            "Least",
            "Note",
            "Offset",
            "OffsetBy",
            "Periodic",
            "Piece",
            "Piecewise",
            "Recur",
            "Ref",
            "RefIn",
            "Reopened",
            "Sealed",
            "Shift",
            "Signed",
            "Snapshot",
            "Source",
            "SpecRevised",
            "SubTodo",
            "Tended",
            "Within",
            "Woven",
        ]
    );
}

fn name(byte: char) -> Hash {
    Hash::new(byte.to_string().repeat(64)).expect("hex")
}

/// §3's new objects print every field in declared order, `''` for an unset
/// `previous`.
#[test]
fn the_new_objects_print_as_declared() {
    let genesis: Envelope<TodoEvent<Todo>> =
        Envelope::Genesis(mk_genesis("suzatary", &"9f".repeat(16)).expect("a genesis"));
    assert_eq!(
        canonical_envelope(&genesis),
        format!("Genesis(label='suzatary', nonce='{}')", "9f".repeat(16))
    );
    assert!(parents_of(&genesis).is_empty());
    assert!(genesis.event().is_none());

    let event = common::events().remove(5);
    let change = Envelope::Change(
        mk_change(name('0'), vec![name('b'), name('a')], event.clone()).expect("a change"),
    );
    assert_eq!(
        canonical_envelope(&change),
        format!(
            "Change(genesis='{}', deps=('{}', '{}'), event={})",
            name('0').as_str(),
            name('a').as_str(),
            name('b').as_str(),
            canonical(&event)
        )
    );
    assert_eq!(
        parents_of(&change),
        vec![name('0'), name('a'), name('b')],
        "a change rests on its genesis and its deps"
    );
    assert_eq!(change.event(), Some(&event));

    let snapshot: Envelope<TodoEvent<Todo>> = Envelope::Snapshot(
        mk_snapshot(name('0'), vec![name('c'), name('a')], None).expect("a snapshot"),
    );
    assert_eq!(
        canonical_envelope(&snapshot),
        format!(
            "Snapshot(genesis='{}', tips=('{}', '{}'), previous='')",
            name('0').as_str(),
            name('a').as_str(),
            name('c').as_str()
        )
    );
    assert!(snapshot.event().is_none());
}

#[test]
fn the_new_constructors_refuse_what_section_3_forbids() {
    let event = common::events().remove(0);
    assert!(mk_genesis("x", "9F".repeat(16).as_str()).is_err());
    assert!(mk_change(name('0'), vec![name('a'), name('a')], event.clone()).is_err());
    assert!(mk_change(name('0'), vec![], event).is_ok());
    assert!(mk_snapshot(name('0'), vec![], None).is_err());
    assert!(mk_snapshot(name('0'), vec![name('a'), name('a')], None).is_err());
}

/// A snapshot's parents are its genesis, its tips and its previous, each
/// once.
#[test]
fn a_snapshot_rests_on_its_genesis_its_tips_and_its_previous() {
    let on = |tips: Vec<Hash>, previous| {
        parents_of::<TodoEvent<Todo>>(&Envelope::Snapshot(
            mk_snapshot(name('0'), tips, previous).expect("a snapshot"),
        ))
    };
    let tips = || vec![name('b'), name('a')];
    assert_eq!(
        on(tips(), Some(name('c'))),
        vec![name('0'), name('a'), name('b'), name('c')]
    );
    assert_eq!(
        on(tips(), Some(name('a'))),
        vec![name('0'), name('a'), name('b')]
    );
    assert_eq!(on(vec![name('0')], None), vec![name('0')]);
}

/// An event's id is the hash of its print, whichever object carries it.
#[test]
fn an_event_id_ignores_where_the_event_was_written() {
    let event = common::events().remove(2);
    let here = mk_change(name('0'), vec![], event.clone()).expect("a change");
    let there = mk_change(name('0'), vec![name('a')], event.clone()).expect("a change");
    assert_eq!(event_id(&here.event), event_id(&there.event));
    assert_ne!(
        seal_hash(&Envelope::Change(here)),
        seal_hash(&Envelope::Change(there))
    );
    assert_ne!(event_id(&event), event_id(&common::events().remove(3)));
}

fn a_name() -> impl Strategy<Value = Hash> {
    "[0-9a-f]{64}".prop_map(|text| Hash::new(text).expect("hex"))
}

fn some_names(size: std::ops::Range<usize>) -> impl Strategy<Value = Vec<Hash>> {
    prop::collection::btree_set(a_name(), size).prop_map(|names| names.into_iter().rev().collect())
}

pub(crate) fn a_new_object() -> impl Strategy<Value = Envelope<TodoEvent<Todo>>> {
    let events = common::events();
    prop_oneof![
        (".{0,12}", "[0-9a-f]{32}").prop_map(|(label, nonce)| Envelope::Genesis(
            mk_genesis(&label, &nonce).expect("a genesis")
        )),
        (a_name(), some_names(0..4), prop::sample::select(events)).prop_map(
            |(genesis, deps, event)| Envelope::Change(
                mk_change(genesis, deps, event).expect("a change")
            )
        ),
        (a_name(), some_names(1..4), prop::option::of(a_name())).prop_map(
            |(genesis, tips, previous)| Envelope::Snapshot(
                mk_snapshot(genesis, tips, previous).expect("a snapshot")
            )
        ),
    ]
}

proptest! {
    #![proptest_config(crate::common::cases::cases(256))]

    /// Law 1 over §3's new objects.
    #[test]
    fn a_new_object_prints_and_parses_back(object in a_new_object()) {
        let text = canonical_envelope(&object);
        let parsed = parse_envelope::<TodoEvent<Todo>>(&Default::default(), &text).expect("a canonical print parses");
        prop_assert_eq!(&parsed, &object);
        prop_assert_eq!(canonical_envelope(&parsed), text);
        prop_assert_eq!(seal_hash(&parsed), seal_hash(&object));
    }
}
