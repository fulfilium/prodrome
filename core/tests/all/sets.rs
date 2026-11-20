//! SPEC law 41, the design's law 12: a set of prodromes is a prodrome.
//!
//! The qualified reference (§7.2): `RefIn(store, entity)` reads the
//! entity's function in the member of the set a host passes that `store`
//! names, as that member reads it; an unqualified `Ref` keeps meaning what
//! it meant; a store or an entity the set does not hold reads `∅`.
//!
//! The set (design §6.3.1), over generated sets of todo prodromes, each a
//! replica of its own genesis: a set is a nest keyed by genesis, so its join
//! reads each member at its genesis as the member reads alone (the
//! DISJOINTNESS law), sets join by union and a set of sets is associative;
//! and its reading is the product of its members' readings, so a qualified
//! reference into one member reads as that member's own view prices it.

use crate::common;

use std::collections::BTreeMap;

use common::terms::{a_spec_onto, acyclic_specs, an_env, moment, ok, TODOS};
use common::{a_log, far, Event};
use prodrome::event::Hash;
use prodrome::fold;
use prodrome::fpl::{self, instant_of, link, link_in, Closed, Env, LinkError, Member, Stores};
use prodrome::nest::{History, Leaf, Level, Nest, Path, Segment, Set};
use prodrome::policy::Everything;
use prodrome::store::Sender;
use prodrome::store::{sync, MemoryStore, Replica};
use prodrome::todo::TodoVocabulary;
use prodrome::view;
use proptest::prelude::*;

use crate::nest_histories::{dag, sub, union, Sub};

/// Two prodromes' names, as their geneses would be.
const HERE: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const THERE: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn name(genesis: &str) -> Hash {
    Hash::new(genesis).expect("a genesis's name")
}

fn readings(linked: &Closed, env: &Env, probes: &[i64]) -> Vec<Option<f64>> {
    probes
        .iter()
        .map(|h| fpl::fulfillment(linked, moment(*h), env))
        .collect()
}

/// `RefIn`'s store obeys `TodoId`'s rule, as a genesis's name does, and its
/// entity is a path segment, in the constructor and so in the parse.
#[test]
fn a_qualified_ref_names_a_store_and_an_entity_or_is_refused() {
    let good = ok(fpl::mk_ref_in(THERE.to_owned(), "Any Key".to_owned()));
    let print = fpl::print_term(&good);
    assert_eq!(print, format!("RefIn(store='{THERE}', entity='Any Key')"));
    assert_eq!(fpl::parse_term(&print).expect("parses"), good);
    for (store, entity) in [("", "a"), ("Todos", "a"), ("todos", ""), ("todos", "a/b")] {
        assert!(fpl::mk_ref_in(store.to_owned(), entity.to_owned()).is_err());
        let print = format!("RefIn(store={store:?}, entity={entity:?})");
        assert!(fpl::parse_term(&print).is_err(), "{print} parsed");
    }
}

/// A store the set does not hold, or an entity its member does not, links
/// to `Absent` and reads `∅` at every instant, under any environment.
#[test]
fn a_qualified_ref_to_what_the_set_lacks_is_absent() {
    let member = Member {
        specs: [("a".to_owned(), ok(fpl::mk_flat(0.5)))].into(),
        env: Env::new(),
    };
    let stores = Stores::new()
        .with(name(THERE), member)
        .named("todos", name(THERE));
    for (store, entity) in [("todos", "b"), (HERE, "a"), ("elsewhere", "a")] {
        let reference = ok(fpl::mk_ref_in(store.to_owned(), entity.to_owned()));
        let linked = link_in(&reference, &BTreeMap::new(), &stores).expect("links");
        assert_eq!(linked.term(), &fpl::mk_absent(), "{store}/{entity}");
    }
    let present = ok(fpl::mk_ref_in("todos".to_owned(), "a".to_owned()));
    let linked = link_in(&present, &BTreeMap::new(), &stores).expect("links");
    assert_eq!(linked.term(), &ok(fpl::mk_flat(0.5)));
    // With no stores at all, `link` itself: every qualified reference is `∅`.
    assert_eq!(
        link(&present, &BTreeMap::new()).expect("links").term(),
        &fpl::mk_absent()
    );
}

/// References that loop through two prodromes are refused as one loop, an
/// entity of another prodrome named `genesis/entity`.
#[test]
fn a_loop_across_prodromes_is_a_cycle() {
    let back = ok(fpl::mk_offset(
        0.5,
        ok(fpl::mk_ref_in(HERE.to_owned(), "a".to_owned())),
    ));
    let there = Member {
        specs: [("b".to_owned(), back)].into(),
        env: Env::new(),
    };
    let forth = ok(fpl::mk_ref_in(THERE.to_owned(), "b".to_owned()));
    let here = Member {
        specs: [("a".to_owned(), forth.clone())].into(),
        env: Env::new(),
    };
    let stores = Stores::new()
        .with(name(HERE), here)
        .with(name(THERE), there);
    let refusal = link_in(&forth, &BTreeMap::new(), &stores).expect_err("a loop");
    assert_eq!(
        refusal,
        LinkError::Cycle(vec![
            format!("{THERE}/b"),
            format!("{HERE}/a"),
            format!("{THERE}/b"),
        ])
    );
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    /// EVERY UNQUALIFIED REF KEEPS MEANING WHAT IT MEANS: a term that holds
    /// no qualified reference links alike under any set of stores.
    #[test]
    fn an_unqualified_term_links_alike_under_any_set(
        term in a_spec_onto(TODOS.to_vec()),
        specs in acyclic_specs(),
        theirs in acyclic_specs(),
        env in an_env(),
    ) {
        let stores = Stores::new()
            .with(name(THERE), Member { specs: theirs, env })
            .named("todos", name(THERE));
        prop_assert_eq!(link_in(&term, &specs, &stores), link(&term, &specs));
    }

    /// A QUALIFIED REF READS ITS ENTITY AS ITS OWN PRODROME DOES: linked
    /// against that member's functions and read under that member's
    /// environment, whatever the environment of the term it lands in. By a
    /// declared name or by the genesis, alike.
    #[test]
    fn a_qualified_ref_reads_what_its_member_reads(
        specs in acyclic_specs(),
        todo in prop::sample::select(TODOS.to_vec()),
        theirs in an_env(),
        ours in an_env(),
        probes in prop::collection::vec(-500i64..500, 6),
    ) {
        let member = Member { specs: specs.clone(), env: theirs.clone() };
        let stores = Stores::new().with(name(THERE), member).named("todos", name(THERE));
        let at_home = readings(&link(&specs[todo], &specs).expect("acyclic"), &theirs, &probes);
        for store in ["todos", THERE] {
            let reference = ok(fpl::mk_ref_in(store.to_owned(), todo.to_owned()));
            let linked = link_in(&reference, &BTreeMap::new(), &stores).expect("acyclic");
            let away = readings(&linked, &ours, &probes);
            prop_assert!(
                at_home.iter().zip(&away).all(|(a, b)| match (a, b) {
                    (Some(a), Some(b)) => (a - b).abs() <= 1e-9,
                    (a, b) => a == b,
                }),
                "{store}: {at_home:?} vs {away:?}"
            );
        }
    }
}

// --- The set: a nest keyed by genesis -----------------------------------------

/// Member `n`'s label, so its genesis is its own.
fn label(n: usize) -> String {
    format!("member-{n}")
}

/// A replica of member `n`'s prodrome holding `log`, written in order: two
/// replicas of one member, one holding a prefix of the other's log, hold a
/// down-set of the other's objects.
fn member(n: usize, log: &[Event]) -> Sub<Event> {
    let member = sub::<Event>(TodoVocabulary::default(), &label(n));
    for event in log {
        member
            .store
            .append(event.clone())
            .expect("a todo event appends");
    }
    member
}

/// The fold a replica holds.
fn folded(replica: &impl Replica<Event>) -> std::sync::Arc<prodrome::registers::Folded<Event>> {
    replica.held().expect("reads").folded
}

fn genesis(member: &Sub<Event>) -> String {
    let dag = dag(&member.store);
    let geneses = dag.geneses();
    assert_eq!(geneses.len(), 1, "a member is one prodrome");
    geneses.into_iter().next().expect("one").into_string()
}

/// A replica holding every member given, synced.
fn set_replica(members: &[Sub<Event>]) -> MemoryStore<Event> {
    if members.is_empty() {
        return MemoryStore::default();
    }
    union(&members.iter().collect::<Vec<_>>())
}

/// The set a replica holds, as a nest.
fn set_of(replica: &impl Replica<Event>) -> Nest {
    let dag = dag(replica);
    let leaf = Leaf::new(dag.clone());
    Set::new(dag, &leaf)
        .nest(&replica.tips().expect("derives"))
        .expect("a set reads")
}

/// A member alone, as its own nest.
fn alone(member: &Sub<Event>) -> Nest {
    Leaf::new(dag(&member.store))
        .nest(&member.store.tips().expect("derives"))
        .expect("a member reads")
}

fn at_genesis(genesis: &str) -> Path {
    Segment::new(genesis).expect("a segment").into()
}

/// One to three members' logs, and three sets of them, each holding of
/// every member nothing or a prefix of its log.
fn sets() -> impl Strategy<Value = (Vec<Vec<Event>>, [Vec<Option<usize>>; 3])> {
    prop::collection::vec(a_log(), 1..=3).prop_flat_map(|logs| {
        let held = |logs: &Vec<Vec<Event>>| {
            logs.iter()
                .map(|log| prop::option::of(0..=log.len()))
                .collect::<Vec<_>>()
        };
        (Just(logs.clone()), [held(&logs), held(&logs), held(&logs)])
    })
}

/// A set holding of each member the prefix `held` says.
fn holding(logs: &[Vec<Event>], held: &[Option<usize>]) -> MemoryStore<Event> {
    let members: Vec<Sub<Event>> = logs
        .iter()
        .zip(held)
        .enumerate()
        .filter_map(|(n, (log, held))| held.map(|k| member(n, &log[..k])))
        .collect();
    set_replica(&members)
}

/// The union of two replicas, neither touched.
fn joined(a: &MemoryStore<Event>, b: &MemoryStore<Event>) -> MemoryStore<Event> {
    let out = MemoryStore::default();
    sync(a, &out, Sender::Stranger).expect("syncs");
    sync(b, &out, Sender::Stranger).expect("syncs");
    out
}

/// The reading of a linked term at instants across the generator's window,
/// and last at [`far`], when every event is in force.
fn sampled(linked: &Closed, env: &Env) -> Vec<Option<f64>> {
    (0..=8)
        .map(|n| common::moment(n * common::WINDOW / 8))
        .chain([far()])
        .map(|t| fpl::fulfillment(linked, instant_of(t), env))
        .collect()
}

fn agree(a: &[Option<f64>], b: &[Option<f64>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() <= 1e-9,
            (a, b) => a == b,
        })
}

/// `RefIn(store, todo)` linked through `stores`, read across the window
/// from a term with no environment of its own.
fn through(stores: &Stores, store: &str, todo: &str) -> Vec<Option<f64>> {
    let reference = ok(fpl::mk_ref_in(store.to_owned(), todo.to_owned()));
    let linked = link_in(&reference, &BTreeMap::new(), stores).expect("links");
    sampled(&linked, &Env::new())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(48))]

    /// DISJOINTNESS. A set is a nest keyed by genesis: its join holds every
    /// object once, and reads at each member's genesis exactly as that
    /// member reads alone, whatever the other members hold.
    #[test]
    fn each_member_reads_in_the_set_as_alone(logs in prop::collection::vec(a_log(), 1..=3)) {
        let members: Vec<Sub<Event>> =
            logs.iter().enumerate().map(|(n, log)| member(n, log)).collect();
        let replica = set_replica(&members);
        let set = set_of(&replica);
        let flat = set.flatten();
        prop_assert_eq!(flat.len(), dag(&replica).objects().len());
        prop_assert!(set.own().is_empty(), "a set has no objects of its own");
        for member in &members {
            let path = at_genesis(&genesis(member));
            prop_assert_eq!(&set.held()[&path], &alone(member));
            prop_assert_eq!(flat.at(&path), alone(member).flatten());
        }
    }

    /// Sets join by union, each member by its own genesis: the set a synced
    /// replica holds is the union of the sets synced into it, so a set of
    /// sets flattens as a nest keyed at the root (a genesis is a global
    /// name, so a member needs no prefix), and associatively, in its
    /// history and in its content name.
    #[test]
    fn a_set_of_sets_is_associative((logs, [a, b, c]) in sets()) {
        let (a, b, c) = (holding(&logs, &a), holding(&logs, &b), holding(&logs, &c));
        let flat = |replica: &MemoryStore<Event>| set_of(replica).flatten();
        prop_assert_eq!(flat(&joined(&a, &b)), flat(&a).union(flat(&b)));
        let left = joined(&joined(&a, &b), &c);
        let right = joined(&a, &joined(&b, &c));
        prop_assert_eq!(set_of(&left).name().clone(), set_of(&right).name().clone());
        let root = Path::root;
        let nested: History<History<History<_>>> = [
            (root(), [(root(), flat(&a)), (root(), flat(&b))].into_iter().collect()),
            (root(), [(root(), flat(&c))].into_iter().collect()),
        ]
        .into_iter()
        .collect();
        let outer_first = nested.clone().join().join();
        let inner_first = nested.fmap(History::join).join();
        prop_assert_eq!(&outer_first, &inner_first);
        prop_assert_eq!(outer_first, flat(&left));
    }

    /// THE READING IS THE PRODUCT. A set's reading is each member's, read
    /// from its own objects alone: a qualified reference into a member
    /// reads, through the set, what it reads through that member alone, and
    /// what that member's own view prices the entity at; by a declared name
    /// as by the genesis. An entity the member lacks, or a member the set
    /// lacks, reads `∅`.
    #[test]
    fn a_set_reads_as_the_product_of_its_members(logs in prop::collection::vec(a_log(), 1..=3)) {
        let members: Vec<Sub<Event>> =
            logs.iter().enumerate().map(|(n, log)| member(n, log)).collect();
        let now = instant_of(far());
        let replica = set_replica(&members);
        let set = fold::stores(&folded(&replica), now, &Everything).expect("reads");
        let first = genesis(&members[0]);
        let named = set.clone().named("todos", name(&first));
        for member in &members {
            let genesis = genesis(member);
            let own = fold::stores(&folded(&member.store), now, &Everything).expect("reads");
            let entries = view::entries(
                &dag(&member.store).nodes().expect("whole"),
                far(),
                &Everything,
            )
            .expect("a view");
            for todo in common::TODOS {
                let through_set = through(&set, &genesis, todo);
                prop_assert!(agree(&through_set, &through(&own, &genesis, todo)));
                if genesis == first {
                    prop_assert!(agree(&through_set, &through(&named, "todos", todo)));
                }
                let entry = entries.iter().find(|entry| entry.key.as_str() == todo);
                let priced = through_set[9];
                match entry {
                    Some(entry) => {
                        let value = entry.price.value.clone().expect("the generator's specs link");
                        prop_assert!(agree(&[priced], &[value]), "{todo}: {priced:?} vs {value:?}");
                    }
                    None => prop_assert!(through_set.iter().all(Option::is_none)),
                }
            }
        }
        let stranger = genesis(&member(9, &[]));
        for todo in common::TODOS {
            prop_assert!(through(&set, &stranger, todo).iter().all(Option::is_none));
        }
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(48))]

    /// A VIEW READS ITS REPLICA AS A SET. In a replica holding two todo
    /// prodromes, a todo of one whose spec is a qualified reference to a
    /// todo of the other prices, in the view, as that todo does in its own
    /// prodrome's view.
    #[test]
    fn a_view_prices_a_qualified_ref_through_its_replicas_set(log in a_log()) {
        let there = member(0, &log);
        let elsewhere = genesis(&there);
        let here = sub::<Event>(TodoVocabulary::default(), &label(1));
        for (n, todo) in (0..).zip(common::TODOS) {
            let spec = ok(fpl::mk_ref_in(elsewhere.clone(), todo.to_owned()));
            let event = common::ok(prodrome::event::mk_spec_revised(
                todo,
                common::moment(n),
                "bassel",
                spec,
                "",
            ));
            here.store.append(event).expect("appends");
        }
        let replica = union(&[&there, &here]);
        let view = |replica: &MemoryStore<Event>| {
            view::entries(&dag(replica).nodes().expect("whole"), far(), &Everything)
                .expect("a view")
        };
        let (both, alone) = (view(&replica), view(&there.store));
        let here_genesis = name(&genesis(&here));
        for todo in common::TODOS {
            let value = |entries: &[view::Entry<Event>], genesis: Option<&Hash>| {
                entries
                    .iter()
                    .find(|entry| entry.key.as_str() == todo && entry.genesis.as_ref() == genesis)
                    .map(|entry| entry.price.value.clone().expect("links"))
            };
            let priced = value(&both, Some(&here_genesis)).expect("a spec revised");
            let at_home = value(&alone, alone.first().and_then(|entry| entry.genesis.as_ref()))
                .flatten();
            prop_assert!(agree(&[priced], &[at_home]), "{todo}: {priced:?} vs {at_home:?}");
        }
    }
}

// --- The motivating use: a proposal priced by the todo it serves --------------

#[path = "../schemas/inbox.rs"]
mod inbox;

use inbox::{Id, Inbox, InboxVocabulary, Says};

/// One thing that happens to the set of an inbox and two todo prodromes.
#[derive(Debug, Clone)]
enum Step {
    /// An event of the served todo, `alpha`, in its own prodrome.
    Served(common::Draft),
    /// An event of any todo in the other todo prodrome.
    Other(common::Draft),
    /// An inbox event about another proposal, or the served one's text.
    Inbox(u8),
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(
        prop_oneof![
            2 => common::a_draft().prop_map(|mut draft| {
                draft.todo = "alpha";
                Step::Served(draft)
            }),
            2 => common::a_draft().prop_map(Step::Other),
            1 => any::<u8>().prop_map(Step::Inbox),
        ],
        1..20,
    )
}

fn proposal(proposal: &str, n: i64, says: Says) -> Inbox {
    Inbox {
        proposal: Id(proposal.to_owned()),
        at: common::moment(n * 3600),
        actor: prodrome::event::Actor::new("bassel").expect("an actor"),
        says,
    }
}

/// The proposal `p`'s price, read through `stores`: its reading priced
/// ([`fold::price`]), linked in the set and read across the window. A
/// proposal that serves nothing has no price, which is not `∅`.
fn price_of(inbox: &Sub<Inbox>, p: &str, stores: &Stores) -> Option<Vec<Option<f64>>> {
    let held = inbox.store.held().expect("reads");
    let stream = held
        .folded
        .entities()
        .find(|(key, _)| key.0 == p)
        .map(|(_, stream)| stream)?;
    let registers = fold::read(stream, Some(instant_of(far())), &Everything);
    let term = fold::price::<Inbox>(&registers).expect("prices")?;
    let linked = link_in(&term, &BTreeMap::new(), stores).expect("links");
    Some(sampled(&linked, &Env::new()))
}

proptest! {
    #![proptest_config(crate::common::cases::cases(48))]

    /// THE MOTIVATING USE. A proposal in an inbox serves `alpha` in the
    /// todo store the host names `todos`, a member of the set beside
    /// another todo prodrome. Read through the set, its price is `alpha`'s
    /// as its own store's view prices it, at every step; it moves when
    /// `alpha`'s history moves, and an event anywhere else (another todo
    /// prodrome, another proposal, the proposal's own text) never moves it.
    #[test]
    fn a_proposal_prices_as_the_todo_it_serves(steps in steps()) {
        let served = sub::<Event>(TodoVocabulary::default(), "served");
        let other = sub::<Event>(TodoVocabulary::default(), "other");
        let inbox = sub::<Inbox>(InboxVocabulary, "inbox");
        let serves = Says::Serves { store: "todos".to_owned(), todo: "alpha".to_owned() };
        inbox.store.append(proposal("p1", 0, serves)).expect("appends");
        let named = genesis(&served);
        let read = |served: &Sub<Event>, other: &Sub<Event>| {
            let todos = union(&[served, other]);
            fold::stores(&folded(&todos), instant_of(far()), &Everything)
                .expect("reads")
                .named("todos", name(&named))
        };
        let mut before = price_of(&inbox, "p1", &read(&served, &other)).expect("p1 serves");
        for (n, step) in (1..).zip(&steps) {
            let at = common::moment(n * 3600);
            match step {
                Step::Served(draft) => drop(served.store.append(draft.at(at)).expect("appends")),
                Step::Other(draft) => drop(other.store.append(draft.at(at)).expect("appends")),
                Step::Inbox(pick) => {
                    let event = match pick % 3 {
                        0 => proposal("p1", n, Says::Proposed(format!("text {pick}"))),
                        1 => proposal("p2", n, Says::Proposed(format!("text {pick}"))),
                        _ => proposal("p2", n, Says::Serves {
                            store: "todos".to_owned(),
                            todo: "beta".to_owned(),
                        }),
                    };
                    inbox.store.append(event).expect("appends");
                }
            }
            let now = price_of(&inbox, "p1", &read(&served, &other)).expect("p1 serves");
            let entries = view::entries(
                &dag(&served.store).nodes().expect("whole"),
                far(),
                &Everything,
            )
            .expect("a view");
            match entries.iter().find(|entry| entry.key.as_str() == "alpha") {
                Some(entry) => {
                    let value = entry.price.value.clone().expect("the generator's specs link");
                    prop_assert!(agree(&[now[9]], &[value]), "{:?} vs {value:?}", now[9]);
                }
                None => prop_assert!(now.iter().all(Option::is_none)),
            }
            if !matches!(step, Step::Served(_)) {
                prop_assert_eq!(&now, &before, "{:?} moved the price", step);
            }
            before = now;
        }
    }
}
