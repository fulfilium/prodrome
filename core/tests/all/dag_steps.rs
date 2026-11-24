//! The DAG as a value: the facts that it keeps across its own steps, and
//! what it says of a print that is not an object.

use std::collections::BTreeSet;

use prodrome::dag::{Dag, Finding, Unread};
use prodrome::event::{canonical_envelope, mk_created, mk_sealed, mk_woven, seal_hash, TodoEvent};
use prodrome::event::{Envelope, Hash};
use prodrome::literal::Datetime;
use prodrome::reference::Todo;
use prodrome::store::Sender;
use proptest::prelude::*;

type Object = Envelope<TodoEvent<Todo>>;

/// Objects as drawn: for each object in turn, picks among the objects
/// before it, which [`realise`] reduces to its parents.
fn a_dag() -> impl Strategy<Value = Vec<Vec<usize>>> {
    prop::collection::vec(prop::collection::vec(0usize..64, 0..3), 1..16)
}

/// The drawn objects, in the order drawn: `Sealed` for one parent or
/// none, and `Woven` for more.
fn realise(drawn: &[Vec<usize>]) -> Vec<(Hash, Object)> {
    let at = Datetime::new(2026, 9, 1, 12, 0, 0, 0).expect("a real instant");
    let mut objects: Vec<(Hash, Object)> = Vec::with_capacity(drawn.len());
    for (i, picks) in drawn.iter().enumerate() {
        let parents: BTreeSet<Hash> = picks
            .iter()
            .filter(|_| i > 0)
            .map(|pick| objects[pick % i].0.clone())
            .collect();
        let event = mk_created(&format!("t{i}"), at, "ada", "", "").expect("valid");
        let object = if parents.len() > 1 {
            mk_woven(parents.into_iter().collect(), Some(event)).expect("distinct parents")
        } else {
            mk_sealed(parents.into_iter().next(), event)
        };
        objects.push((seal_hash(&object), object));
    }
    objects
}

/// One of the DAG's own operations that change its objects, over the
/// drawn objects by their places.
#[derive(Debug, Clone)]
enum Step {
    Extend(Vec<usize>),
    Grow(Vec<usize>),
    Forget(usize),
    Settle,
    Interior,
}

fn a_step() -> impl Strategy<Value = Step> {
    let some = || prop::collection::vec(0usize..64, 0..6);
    prop_oneof![
        some().prop_map(Step::Extend),
        some().prop_map(Step::Grow),
        (0usize..64).prop_map(Step::Forget),
        Just(Step::Settle),
        Just(Step::Interior),
    ]
}

proptest! {
    #![proptest_config(crate::common::cases::cases(256))]

    /// Law 59: the kept tips and the kept order equal those of a DAG
    /// read fresh from the same objects, after any sequence of `extend`,
    /// `grow`, `forget`, `settle` and `interior`, with each fact asked
    /// for before and after every step.
    #[test]
    fn a_kept_fact_is_the_fresh_one(
        drawn in a_dag(),
        steps in prop::collection::vec(a_step(), 0..12),
    ) {
        let objects = realise(&drawn);
        let pick = |at: &usize| objects[at % objects.len()].clone();
        let mut dag: Dag<TodoEvent<Todo>> = Dag::from_iter([]);
        for step in steps {
            let _ = (dag.tips(), dag.linearise().map(<[Hash]>::len));
            match step {
                Step::Extend(picks) => dag.extend(picks.iter().map(pick)),
                Step::Grow(picks) => {
                    dag.grow(picks.iter().map(pick).collect());
                }
                Step::Forget(at) => {
                    dag.forget(&pick(&at).0);
                }
                Step::Settle => dag.settle(),
                Step::Interior => dag = dag.interior(),
            }
            let fresh: Dag<TodoEvent<Todo>> = dag.objects().clone().into_iter().collect();
            prop_assert_eq!(dag.tips(), fresh.tips());
            prop_assert_eq!(dag.linearise(), fresh.linearise());
        }
    }
}

#[test]
fn linearise_refuses_a_missing_parent() {
    let at = Datetime::new(2026, 9, 1, 12, 0, 0, 0).expect("a real instant");
    let event = mk_created("alpha", at, "bassel", "", "").expect("valid");
    let absent = Hash::new("a".repeat(64)).expect("hex");
    let orphaned: Envelope<TodoEvent<Todo>> = mk_sealed(Some(absent.clone()), event);
    let name = seal_hash(&orphaned);
    let dag: Dag<TodoEvent<Todo>> = [(name.clone(), orphaned)].into_iter().collect();
    assert_eq!(
        dag.linearise().unwrap_err().to_string(),
        format!(
            "missing object {}, named as a parent by {}",
            absent.as_str(),
            name.as_str()
        )
    );
}

/// A print that is not an object, and the chain on it: the history
/// leaves out the print and both objects above it, and `excluded` says
/// so, with the parse's own refusal, as `verify` reports it.
#[test]
fn excluded_names_an_unread_print_and_what_rests_on_it() {
    let at = Datetime::new(2026, 9, 1, 12, 0, 0, 0).expect("a real instant");
    let junk = b"Sealed(prev=None, event=Nonsense())".to_vec();
    let unread = Hash::of_bytes(&junk);
    let first: Envelope<TodoEvent<Todo>> = mk_sealed(
        Some(unread.clone()),
        mk_created("alpha", at, "bassel", "", "").expect("valid"),
    );
    let second: Envelope<TodoEvent<Todo>> = mk_sealed(
        Some(seal_hash(&first)),
        mk_created("beta", at, "bassel", "", "").expect("valid"),
    );
    let print = |object: &Envelope<TodoEvent<Todo>>| {
        (
            seal_hash(object),
            Ok(canonical_envelope(object).into_bytes()),
        )
    };
    let dag: Dag<TodoEvent<Todo>> = Dag::from_prints(
        &prodrome::todo::TodoVocabulary::default(),
        [(unread.clone(), Ok(junk)), print(&first), print(&second)],
        Sender::Stranger,
    );
    let excluded = dag.excluded();
    assert_eq!(excluded.len(), 1);
    assert_eq!(excluded[0].name, unread);
    assert!(matches!(excluded[0].why, Some(Unread::Unparsed(_))));
    assert_eq!(
        excluded[0].resting,
        [seal_hash(&first), seal_hash(&second)].into()
    );
    assert!(dag.interior().objects().is_empty());
    let findings = dag.verify(&prodrome::policy::Everything);
    assert!(matches!(&findings[0], Finding::Unread { name, .. } if *name == unread));
    assert!(matches!(&findings[1], Finding::Broken { at, why: Some(_) } if *at == unread));
}
