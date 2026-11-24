//! The reference policy and the policy under which everything binds
//! (SPEC §5).

use prodrome::event::Hash;
use prodrome::event::{mk_completed, mk_created, Actor, TodoEvent};
use prodrome::literal::Datetime;
use prodrome::policy::{Everything, Policy, Standing, Untrusted};
use prodrome::reference::{mk_authored, Todo};

type Event = TodoEvent<Todo>;

fn at() -> Datetime {
    Datetime::new(2026, 9, 6, 7, 3, 0, 0).expect("a real instant")
}

fn record(actor: &str) -> Event {
    mk_authored(
        "alpha",
        at(),
        actor,
        "todo",
        at(),
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
    .expect("valid")
}

/// Neither policy here reads the object's name.
fn object() -> Hash {
    Hash::new("0".repeat(64)).expect("a name")
}

fn roster() -> Untrusted {
    Untrusted::of([Actor::new("triage").expect("valid")])
}

#[test]
fn a_named_actors_lifecycle_event_claims_and_its_record_binds() {
    let policy = roster();
    let claimed: Event = mk_completed("alpha", at(), "triage", "").expect("valid");
    assert_eq!(policy.standing(&object(), &claimed), Standing::Claims);
    assert_eq!(
        policy.standing(&object(), &record("triage")),
        Standing::Binds
    );
    assert_eq!(
        policy.standing(
            &object(),
            &mk_completed::<Todo>("alpha", at(), "bassel", "").expect("valid")
        ),
        Standing::Binds
    );
}

#[test]
fn a_record_that_binds_can_still_not_be_the_hosts_own_word() {
    let policy = roster();
    assert!(policy.standing(&object(), &record("triage")).binds());
    assert!(
        !policy.confirms(&object(), &record("triage")),
        "it binds, and it is still the agent's"
    );
    assert!(policy.confirms(&object(), &record("bassel")));
}

#[test]
fn an_empty_roster_is_the_policy_that_binds_everything() {
    let none = Untrusted::none();
    for event in [
        record("triage"),
        mk_completed("alpha", at(), "triage", "").expect("valid"),
        mk_created("alpha", at(), "triage", "", "").expect("valid"),
    ] {
        assert_eq!(
            none.standing(&object(), &event),
            Everything.standing(&object(), &event)
        );
        assert_eq!(
            none.confirms(&object(), &event),
            Everything.confirms(&object(), &event)
        );
    }
}

#[test]
fn a_claiming_event_is_never_the_hosts_own_word() {
    let policy = roster();
    let claimed: Event = mk_completed("alpha", at(), "triage", "").expect("valid");
    assert!(policy.standing(&object(), &claimed).claims() && !policy.confirms(&object(), &claimed));
}
