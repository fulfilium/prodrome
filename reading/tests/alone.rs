//! A program whose only crates of this workspace are `prodrome-data`,
//! `prodrome-dag`, `prodrome-term`, `prodrome-fpl`, `prodrome-todo` and
//! `prodrome-reading`: it writes the events of a todo, folds them, and reads
//! the todo's price and its row in a list. It opens no store.
//!
//! The test builds only if these are the crates that `prodrome-reading` needs,
//! because an integration test names only the dependencies of the package.

use prodrome_dag::dag::Dag;
use prodrome_data::literal::Datetime;
use prodrome_data::policy::Everything;
use prodrome_fpl::fpl::mk_flat;
use prodrome_reading::view::entries;
use prodrome_todo::event::{mk_created, mk_sealed, mk_spec_revised, seal_hash, TodoEvent};
use prodrome_todo::reference::Todo;

type Event = TodoEvent<Todo>;

fn at(day: u32) -> Datetime {
    Datetime::new(2026, 9, day, 9, 0, 0, 0).expect("a real instant")
}

/// The events of one writer, each sealed on the one before it.
fn chain(events: Vec<Event>) -> Dag<Event> {
    let mut previous = None;
    let mut objects = Vec::new();
    for event in events {
        let envelope = mk_sealed(previous, event);
        let name = seal_hash(&envelope);
        previous = Some(name.clone());
        objects.push((name, envelope));
    }
    objects.into_iter().collect()
}

/// A todo with a flat price reads as one open row that holds that price.
#[test]
fn a_todo_with_a_flat_price_is_one_open_row_that_holds_the_price() {
    let events: Vec<Event> = vec![
        mk_created("todo-1", at(1), "ada", "publish the crate", "").expect("valid"),
        mk_spec_revised(
            "todo-1",
            at(1),
            "ada",
            mk_flat(0.25).expect("in [0, 1]"),
            "",
        )
        .expect("valid"),
    ];
    let nodes = chain(events).nodes().expect("a chain");
    let rows = entries(&nodes, at(9), &Everything).expect("the log folds");
    let [row] = &rows[..] else {
        panic!("one todo, one row")
    };
    assert!(row.is_open());
    assert_eq!(row.state(), "open");
    assert_eq!(row.value(), Ok(Some(0.25)));
}
