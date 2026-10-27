//! A SCHEMA AS DATA (`docs/design-register-types.md` §5, §6.0;
//! `prodrome::declared`), law-tested.
//!
//! - A DIFFERENTIAL: the proposal schema written in Rust and the same
//!   schema declared as data (`../schemas/proposal.rs`) read every generated
//!   history identically. Each event prints the same at both, so every
//!   object is the same bytes under the same name, and every register's
//!   frontier and reading, at every moment, are the same writes. Appended
//!   through a store, each event is written, refused or answered as a twin
//!   alike, and a sync carries the declared store's objects as any.
//! - Law 1, FREE, and the route's agreement, over the declared schema, by
//!   the functions `schema_laws.rs` states them with.
//! - Law 2, ORDER: each declared order is reflexive, antisymmetric and
//!   transitive over generated values, and the declared machine is the Rust
//!   one, pair by pair.
//! - ADMISSION: a generated lawful schema is admitted, in its canonical
//!   print, which parses and prints back the same; a generated schema with
//!   one defect is refused under exactly the law that defect breaks.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use prodrome::dag::Dag;
use prodrome::declared::{
    Datum, Declaration, Declared, Event, Field as FieldForm, Form, Law, Poset, Register, Type,
    Write,
};
use prodrome::event::{canonical, parse_event, Actor, Hash};
use prodrome::fold::{read, Order, Product};
use prodrome::fpl::{instant_of, Instant};
use prodrome::literal::Datetime;
use prodrome::policy::Everything;
use prodrome::registers::{fold, Folded};
use prodrome::schema::Schema;
use prodrome::store::{sync, EventStore, MemoryStore, Replica};
use proptest::prelude::*;
use sha2::{Digest, Sha256};

use crate::common::draw::{a_draw, history, Draw};
use crate::schema_laws::{free, routed};

#[path = "../schemas/proposal.rs"]
pub(crate) mod proposal;

use proposal::{a_proposal, Field, Id, Proposal, Says, State, DECLARED, FIELDS, STATES};

fn declaration() -> Declaration {
    proposals().clone()
}

/// The proposal schema as data, admitted once.
pub(crate) fn proposals() -> &'static Declaration {
    static ADMITTED: OnceLock<Declaration> = OnceLock::new();
    ADMITTED.get_or_init(|| Declaration::admit(DECLARED).expect("the proposal schema is lawful"))
}

/// The `n`th event of a generated sub-history at the declared proposal
/// schema, of a kind `pick` chooses: the nest's laws ask nothing of a
/// schema, so a declared one is held as any other.
pub(crate) fn a_declared_proposal(n: usize, pick: u8) -> Declared {
    let says = match pick % 4 {
        0 => Says::Proposed(format!("text {n}")),
        1 => Says::Moved(STATES[usize::from(pick / 4) % STATES.len()]),
        2 => Says::Attempted(i64::from(pick)),
        _ => Says::Tagged(vec![format!("t{}", pick % 3)]),
    };
    let proposal = Proposal {
        proposal: Id(["p-1", "p-2"][usize::from(pick / 32) % 2].to_owned()),
        at: crate::common::moment(i64::try_from(n).expect("small") * 60),
        actor: Actor::new("ana").expect("an actor"),
        says,
    };
    declared(proposals(), &proposal)
}

fn day(day: u32) -> Datetime {
    Datetime::new(2026, 10, day, 12, 0, 0, 0).expect("a real instant")
}

/// The same event at the declared schema: its print, parsed there.
fn declared(schema: &Declaration, event: &Proposal) -> Declared {
    Declared::from_value(schema, &event.to_value()).expect("a proposal event is a declared one")
}

/// Every entity's registers, by key and register NAME: each frontier and
/// its reading, by object names, at `at`.
type Named = BTreeMap<(Option<Hash>, String), Vec<(String, Vec<Hash>, Vec<Hash>)>>;

fn named<E: Schema>(
    state: &Folded<E>,
    at: Option<Instant>,
    name: impl Fn(E::Register) -> String,
) -> Named
where
    E::Key: AsRef<str>,
{
    let mut out = BTreeMap::new();
    for (genesis, prodrome) in state.prodromes() {
        for (key, stream) in prodrome {
            let registers = read(stream, at, &Everything);
            let readings = registers
                .registers()
                .into_iter()
                .map(|register| {
                    let reading = registers.reading(register);
                    (
                        name(register),
                        registers.frontier(register).names(),
                        reading.iter().map(|stamp| stamp.name.clone()).collect(),
                    )
                })
                .filter(|(_, frontier, _)| !frontier.is_empty())
                .collect();
            out.insert((genesis.clone(), key.as_ref().to_owned()), readings);
        }
    }
    out
}

/// The differential at one draw: the same objects, read the same.
fn differential(draw: &Draw<Proposal>) -> Result<(), TestCaseError> {
    let schema = declaration();
    let twins: Vec<(Declared, u64)> = draw
        .events
        .iter()
        .map(|(event, bits)| (declared(&schema, event), *bits))
        .collect();
    for ((rust, _), (data, _)) in draw.events.iter().zip(&twins) {
        prop_assert_eq!(canonical(rust), canonical(data), "one print");
        prop_assert_eq!(
            &parse_event::<Declared>(&schema, &canonical(rust)).expect("parses"),
            data
        );
    }
    let rust: Dag<Proposal> = history(&draw.events).into_iter().collect();
    let data: Dag<Declared> = history(&twins).into_iter().collect();
    prop_assert_eq!(
        rust.objects().keys().collect::<Vec<_>>(),
        data.objects().keys().collect::<Vec<_>>(),
        "one object set"
    );
    let (rust, data) = (
        fold(&rust.nodes().expect("a DAG")),
        fold(&data.nodes().expect("a DAG")),
    );
    for at in [None, Some(day(2)), Some(day(4))] {
        let at = at.map(instant_of);
        prop_assert_eq!(
            named(&rust, at, |field: Field| field.as_str().to_owned()),
            named(&data, at, |slot| schema.register_name(slot).to_owned()),
            "read at {:?}",
            at
        );
    }
    Ok(())
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Directories removed when dropped.
struct Scratch(Vec<PathBuf>);

impl Scratch {
    fn dir(&mut self) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "prodrome-declared-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        self.0.push(root.clone());
        root
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for root in &self.0 {
            let _ = fs::remove_dir_all(root);
        }
    }
}

/// The differential through the store: appended in turn to a Rust store
/// and to a store opened at the declaration, from one genesis, every event
/// is written as the same object, refused alike (an inflationary register
/// falling) or answered alike as a twin; the stores end reading the same,
/// and a sync carries the declared store into a replica in memory at the
/// declaration, which reads as it does.
fn through_stores(events: &[Proposal]) -> Result<(), TestCaseError> {
    let schema = declaration();
    let mut scratch = Scratch(Vec::new());
    let rust = EventStore::<Proposal, Everything>::new(scratch.dir(), Everything);
    let data = EventStore::<Declared, Everything>::at(scratch.dir(), Everything, schema.clone());
    let genesis = rust.init("proposals").expect("begins");
    let print = Replica::print(&rust, &genesis).expect("held");
    data.receive([genesis].into(), &|_| Some(print.clone()))
        .expect("a genesis is any schema's");
    for event in events {
        let written = rust.append(event.clone()).map_err(|_| ());
        let twin = data.append(declared(&schema, event)).map_err(|_| ());
        prop_assert_eq!(&written, &twin, "{:?}", event);
    }
    prop_assert_eq!(rust.tips().expect("tips"), data.tips().expect("tips"));
    let (rust, data_read) = (rust.folded().expect("folds"), data.folded().expect("folds"));
    prop_assert_eq!(
        named(&rust, None, |field: Field| field.as_str().to_owned()),
        named(&data_read, None, |slot| schema
            .register_name(slot)
            .to_owned())
    );
    let memory = MemoryStore::<Declared>::at(schema.clone());
    sync(&data, &memory).expect("syncs");
    prop_assert_eq!(memory.tips().expect("tips"), data.tips().expect("tips"));
    prop_assert_eq!(
        named(&memory.held().expect("holds").folded, None, |slot| schema
            .register_name(slot)
            .to_owned()),
        named(&data_read, None, |slot| schema
            .register_name(slot)
            .to_owned())
    );
    prop_assert!(data.verify().is_empty(), "{:?}", data.verify());
    Ok(())
}

/// A generated value of a declared type.
fn a_datum(ty: &Type) -> BoxedStrategy<Datum> {
    match ty {
        Type::Text => prop::sample::select(vec!["x", "y", "z"])
            .prop_map(|text| Datum::Text(text.to_owned()))
            .boxed(),
        Type::Integer => (-3..4i64).prop_map(Datum::Integer).boxed(),
        Type::Instant => (1..4u32).prop_map(|d| Datum::Instant(day(d))).boxed(),
        Type::Reference => (0..3u8)
            .prop_map(|n| Datum::Reference(Hash::new(format!("{n:064x}")).expect("a name")))
            .boxed(),
        Type::Enum(alternatives) => prop::sample::select(alternatives.clone())
            .prop_map(Datum::Alternative)
            .boxed(),
        Type::List(item) => prop::collection::vec(a_datum(item), 0..3)
            .prop_map(Datum::List)
            .boxed(),
    }
}

/// One value, as a register's order sees values: a list is a set.
fn same(a: &Datum, b: &Datum) -> bool {
    match (a, b) {
        (Datum::List(a), Datum::List(b)) => {
            a.iter().collect::<BTreeSet<_>>() == b.iter().collect::<BTreeSet<_>>()
        }
        _ => a == b,
    }
}

/// Law 2 over one register's values.
fn partial_order(
    schema: &Declaration,
    register: &str,
    values: &[Datum],
) -> Result<(), TestCaseError> {
    let slot = schema.register(register).expect("a register");
    let le = |a: &Datum, b: &Datum| schema.value(slot, a).le(&schema.value(slot, b));
    for a in values {
        prop_assert!(le(a, a), "reflexive");
        for b in values {
            if le(a, b) && le(b, a) {
                prop_assert!(same(a, b), "antisymmetric: {:?} {:?}", a, b);
            }
            for c in values {
                if le(a, b) && le(b, c) {
                    prop_assert!(le(a, c), "transitive");
                }
            }
        }
    }
    Ok(())
}

// --- admission ---------------------------------------------------------------

const NAMES: [&str; 4] = ["Alpha", "Beta", "Delta", "Gamma"];
const EXTRAS: [&str; 3] = ["x_a", "x_b", "x_c"];

fn a_type() -> impl Strategy<Value = Type> {
    let simple = prop_oneof![
        Just(Type::Text),
        Just(Type::Integer),
        Just(Type::Instant),
        Just(Type::Reference),
        prop::sample::subsequence(vec!["p", "q", "r", "s"], 1..4)
            .prop_map(|alts| Type::Enum(alts.into_iter().map(str::to_owned).collect())),
    ];
    prop_oneof![
        3 => simple.clone(),
        1 => simple.prop_map(|item| Type::List(Box::new(item))),
    ]
}

/// A register over one field of type `ty`, in an order that reads it, and
/// inflationary only where that order has a bottom.
fn a_register(name: String, write: Write, ty: &Type, choice: u8, inflationary: bool) -> Register {
    let (order, inflationary) = match (ty, choice % 2) {
        (Type::Integer, 1) => (Poset::Total, false),
        (Type::List(_), 1) => (Poset::Inclusion, inflationary),
        (Type::Enum(states), 1) => {
            // Covers that only climb in the alternatives' order are
            // acyclic: `choice`'s bits pick which; an inflationary machine
            // also climbs from the first to each other, its bottom.
            let mut covers = Vec::new();
            let mut bit = 1;
            for i in 0..states.len() {
                for j in i + 1..states.len() {
                    if u32::from(choice) >> bit & 1 == 1 || (inflationary && i == 0) {
                        covers.push((states[i].clone(), states[j].clone()));
                    }
                    bit += 1;
                }
            }
            (Poset::Machine(covers), inflationary)
        }
        _ => (Poset::Discrete, false),
    };
    Register {
        name,
        order,
        inflationary,
        writes: vec![write],
    }
}

/// A lawful form: events with the key, a stamp and extra typed fields in a
/// shuffled order, and registers over some of the extras.
fn a_form() -> impl Strategy<Value = Form> {
    let event = (
        prop::collection::vec(a_type(), 0..3),
        any::<u64>(),
        prop::collection::vec((any::<bool>(), any::<u8>(), any::<bool>()), 3),
    );
    (
        prop::sample::select(vec!["door", "proposal", "thing"]),
        prop::sample::subsequence(NAMES.to_vec(), 1..4),
        prop::collection::vec(event, 4),
    )
        .prop_map(|(key, names, events)| {
            let mut form = Form {
                key: key.to_owned(),
                events: Vec::new(),
                registers: Vec::new(),
            };
            for (name, (types, shuffle, registers)) in names.into_iter().zip(events) {
                let mut fields = vec![
                    FieldForm {
                        name: key.to_owned(),
                        ty: Type::Text,
                    },
                    FieldForm {
                        name: "at".to_owned(),
                        ty: Type::Instant,
                    },
                    FieldForm {
                        name: "actor".to_owned(),
                        ty: Type::Text,
                    },
                ];
                for (extra, ty) in EXTRAS.iter().zip(&types) {
                    fields.push(FieldForm {
                        name: (*extra).to_owned(),
                        ty: ty.clone(),
                    });
                }
                let n = fields.len();
                fields.rotate_left(usize::try_from(shuffle % n as u64).expect("small"));
                for ((extra, ty), (registered, choice, inflationary)) in
                    EXTRAS.iter().zip(&types).zip(registers)
                {
                    if registered {
                        let write = Write {
                            event: name.to_owned(),
                            field: (*extra).to_owned(),
                        };
                        let register = format!("r_{}_{extra}", name.to_lowercase());
                        form.registers
                            .push(a_register(register, write, ty, choice, inflationary));
                    }
                }
                form.events.push(Event {
                    name: name.to_owned(),
                    fields,
                });
            }
            form
        })
}

/// One defect, and the law it breaks: the text to admit.
fn broken(form: &Form, defect: u8, pick: usize) -> (String, Law) {
    let mut form = form.clone();
    let event = pick % form.events.len();
    let first = form.events[event].name.clone();
    let key = form.key.clone();
    let drop_field = |form: &mut Form, name: &str| {
        form.events[event].fields.retain(|field| field.name != name);
    };
    let retype = |form: &mut Form, name: &str, ty: Type| {
        for field in &mut form.events[event].fields {
            if field.name == name {
                field.ty = ty.clone();
            }
        }
    };
    let add_field = |form: &mut Form, name: &str, ty: Type| {
        form.events[event].fields.push(FieldForm {
            name: name.to_owned(),
            ty,
        });
    };
    let law = match defect % 8 {
        0 => {
            let text = form.print();
            return (text[..text.len() / 2].to_owned(), Law::Grammar);
        }
        1 => {
            let text = form.print();
            let text = if pick % 2 == 0 {
                text.replacen("Schema(key=", "Schema(", 1)
            } else {
                format!("{text} ")
            };
            return (text, Law::Canonical);
        }
        2 => {
            match pick % 4 {
                0 => form.events.push(form.events[event].clone()),
                1 => {
                    let field = form.events[event].fields[0].clone();
                    form.events[event].fields.push(field);
                }
                2 => form.events[event].name = "Change".to_owned(),
                _ => {
                    let alternatives = Type::Enum(vec!["p".to_owned(), "p".to_owned()]);
                    add_field(&mut form, "dup", alternatives);
                }
            }
            Law::Unique
        }
        3 => {
            match pick % 3 {
                0 => drop_field(&mut form, &key),
                1 => retype(&mut form, &key, Type::Integer),
                _ => form.key = "nothing".to_owned(),
            }
            Law::Key
        }
        4 => {
            if pick % 2 == 0 {
                drop_field(&mut form, "at");
            } else {
                retype(&mut form, "actor", Type::Reference);
            }
            Law::Stamp
        }
        5 => {
            let (field, order) = match pick % 3 {
                0 => ("nofield".to_owned(), Poset::Discrete),
                1 => (key.clone(), Poset::Total),
                _ => (key.clone(), Poset::Machine(vec![])),
            };
            form.registers.push(Register {
                name: "zz_typed".to_owned(),
                order,
                inflationary: false,
                writes: vec![Write {
                    event: first,
                    field,
                }],
            });
            Law::Typed
        }
        6 => {
            add_field(
                &mut form,
                "cyc",
                Type::Enum(vec!["p".to_owned(), "q".to_owned()]),
            );
            let pairs = [("p", "q"), ("q", "p")];
            let covers = if pick % 2 == 0 {
                &pairs[..]
            } else {
                &[("q", "q")][..]
            };
            form.registers.push(Register {
                name: "zz_cycle".to_owned(),
                order: Poset::Machine(
                    covers
                        .iter()
                        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                        .collect(),
                ),
                inflationary: false,
                writes: vec![Write {
                    event: first,
                    field: "cyc".to_owned(),
                }],
            });
            Law::PartialOrder
        }
        _ => {
            let (ty, order) = if pick % 2 == 0 {
                (Type::Integer, Poset::Total)
            } else {
                (
                    Type::Enum(vec!["p".to_owned(), "q".to_owned(), "r".to_owned()]),
                    Poset::Machine(vec![
                        ("p".to_owned(), "r".to_owned()),
                        ("q".to_owned(), "r".to_owned()),
                    ]),
                )
            };
            add_field(&mut form, "low", ty);
            form.registers.push(Register {
                name: "zz_bottom".to_owned(),
                order,
                inflationary: true,
                writes: vec![Write {
                    event: first,
                    field: "low".to_owned(),
                }],
            });
            Law::Bottom
        }
    };
    (form.print(), law)
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    #[test]
    fn a_declared_schema_reads_every_history_as_its_rust_twin(draw in a_draw(a_proposal())) {
        differential(&draw)?;
    }

    #[test]
    fn a_declared_store_appends_as_its_rust_twin(events in prop::collection::vec(a_proposal(), 0..12)) {
        through_stores(&events)?;
    }

    #[test]
    fn a_declared_reading_is_a_function_of_the_object_set(draw in a_draw(a_proposal())) {
        let schema = declaration();
        let draw = Draw {
            events: draw.events.iter().map(|(e, bits)| (declared(&schema, e), *bits)).collect(),
            shuffle: draw.shuffle,
        };
        free(&draw)?;
        for (event, _) in &draw.events {
            routed(event)?;
        }
    }

    #[test]
    fn every_declared_order_is_a_partial_order(
        attempts in prop::collection::vec(a_datum(&Type::Integer), 1..5),
        states in prop::collection::vec(a_datum(&Type::Enum(STATES.iter().map(|s| s.as_str().to_owned()).collect())), 1..5),
        tags in prop::collection::vec(a_datum(&Type::List(Box::new(Type::Text))), 1..5),
        text in prop::collection::vec(a_datum(&Type::Text), 1..5),
    ) {
        let schema = declaration();
        partial_order(&schema, "attempts", &attempts)?;
        partial_order(&schema, "state", &states)?;
        partial_order(&schema, "tags", &tags)?;
        partial_order(&schema, "text", &text)?;
    }

    #[test]
    fn a_lawful_schema_is_admitted_in_its_canonical_print(form in a_form()) {
        let text = form.print();
        let admitted = Declaration::admit(&text);
        prop_assert!(admitted.is_ok(), "{:?}\n{}", admitted, text);
        let admitted = admitted.expect("admitted");
        prop_assert_eq!(admitted.text(), text.as_str());
        prop_assert_eq!(Form::parse(&text).expect("parses").print(), text.clone());
        let digest: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        prop_assert_eq!(admitted.name().as_str(), digest.as_str());
        // Law 2 over every generated machine, exhaustively over its states.
        for register in &form.registers {
            if let Poset::Machine(_) = register.order {
                let write = &register.writes[0];
                let event = form.events.iter().find(|e| e.name == write.event).expect("an event");
                let field = event.fields.iter().find(|f| f.name == write.field).expect("a field");
                let Type::Enum(states) = &field.ty else { panic!("a machine reads an enum") };
                let states: Vec<Datum> = states.iter().cloned().map(Datum::Alternative).collect();
                partial_order(&admitted, &register.name, &states)?;
            }
        }
    }

    #[test]
    fn a_broken_schema_is_refused_under_the_law_it_breaks(
        form in a_form(),
        defect in any::<u8>(),
        pick in any::<usize>(),
    ) {
        let (text, law) = broken(&form, defect, pick);
        let refused = Declaration::admit(&text).map(|_| ());
        prop_assert_eq!(refused.map_err(|refusal| refusal.law), Err(law), "{}", text);
    }
}

/// The declared machine is the Rust one, pair by pair.
#[test]
fn the_declared_machine_is_the_rust_one() {
    let schema = declaration();
    let slot = schema.register("state").expect("a register");
    for a in STATES {
        for b in STATES {
            let (da, db) = (
                Datum::Alternative(a.as_str().to_owned()),
                Datum::Alternative(b.as_str().to_owned()),
            );
            assert_eq!(
                schema.value(slot, &da).le(&schema.value(slot, &db)),
                a.le(&b),
                "{a:?} ≤ {b:?}"
            );
        }
    }
    assert_eq!(
        schema
            .registers()
            .map(|slot| schema.register_name(slot))
            .collect::<Vec<_>>(),
        FIELDS.map(Field::as_str)
    );
    assert!(schema.inflationary(slot));
}

/// The boundary is the declaration's: an event it does not name, a field of
/// the wrong type, or one missing, does not parse.
#[test]
fn a_declared_store_parses_only_its_own_events() {
    let schema = declaration();
    for text in [
        "Opened(pr='a', at=datetime(2026, 10, 1, 12, 0), actor='ana', title='t')",
        "Moved(proposal='a', at=datetime(2026, 10, 1, 12, 0), actor='ana', state='lost')",
        "Attempted(proposal='a', at=datetime(2026, 10, 1, 12, 0), actor='ana', attempt='one')",
        "Proposed(proposal='a', at=datetime(2026, 10, 1, 12, 0), actor='ana')",
        "Tagged(proposal='a', at=datetime(2026, 10, 1, 12, 0), actor='', tags=())",
    ] {
        assert!(parse_event::<Declared>(&schema, text).is_err(), "{text}");
    }
    let moved =
        "Moved(proposal='a', at=datetime(2026, 10, 1, 12, 0, 0), actor='ana', state='sent')";
    let event = parse_event::<Declared>(&schema, moved).expect("parses");
    assert_eq!(canonical(&event), moved);
}

/// Law 4 at a declared store: a write that would take an inflationary
/// register below, or beside, the reading it supersedes is refused, by the
/// append, before any object exists; one that climbs is written.
#[test]
fn a_declared_inflationary_register_refuses_a_fall() {
    let schema = declaration();
    let mut scratch = Scratch(Vec::new());
    let store = EventStore::<Declared, Everything>::at(scratch.dir(), Everything, schema.clone());
    store.init("proposals").expect("begins");
    let event = |says| {
        declared(
            &schema,
            &Proposal {
                proposal: Id("p-1".to_owned()),
                at: day(1),
                actor: Actor::new("ana").expect("an actor"),
                says,
            },
        )
    };
    let tags = |tags: &[&str]| Says::Tagged(tags.iter().map(|t| (*t).to_owned()).collect());
    store
        .append(event(Says::Moved(State::Failed)))
        .expect("climbs");
    let held = store.tips().expect("tips");
    assert!(
        store.append(event(Says::Moved(State::Unknown))).is_err(),
        "beside"
    );
    assert!(
        store.append(event(Says::Moved(State::Standing))).is_err(),
        "below"
    );
    assert_eq!(store.tips().expect("tips"), held, "nothing written");
    store
        .append(event(Says::Moved(State::Sent)))
        .expect("climbs");
    store.append(event(tags(&["a"]))).expect("climbs");
    assert!(store.append(event(tags(&["b"]))).is_err(), "beside");
    store.append(event(tags(&["a", "b"]))).expect("climbs");
    // Not inflationary: an attempt count may go down, a text may change.
    store.append(event(Says::Attempted(3))).expect("written");
    store.append(event(Says::Attempted(1))).expect("written");
}
