//! A program whose only crates of this workspace are `prodrome-data`,
//! `prodrome-term` and `prodrome-todo`: it writes a todo with a decaying
//! price, prints its events and an object that holds one, parses them back
//! and names the object. It evaluates nothing.

use prodrome_data::change::mk_change;
use prodrome_data::event::{
    canonical, canonical_envelope, parse_envelope, parse_event, seal_hash, Envelope, Hash,
};
use prodrome_data::genesis::mk_genesis;
use prodrome_data::instant::instant_of;
use prodrome_data::literal::Datetime;
use prodrome_term::fpl::{delta_from_hours, mk_decay, parse_term, print_term};
use prodrome_todo::event::{mk_completed, mk_created, mk_spec_revised, TodoEvent};
use prodrome_todo::reference::Todo;
use prodrome_todo::todo::TodoVocabulary;

type Event = TodoEvent<Todo>;

fn at(day: u32) -> Datetime {
    Datetime::new(2026, 9, day, 9, 0, 0, 0).expect("a real instant")
}

/// A todo's events print as their constructors, and each print parses back
/// to the event, with a spec that decays toward a deadline.
#[test]
fn a_todo_with_a_decaying_price_prints_and_parses_back() {
    let deadline = instant_of(at(15));
    let spec = mk_decay(0.55, 0.05, deadline, delta_from_hours(72.0), None).expect("a valid decay");
    assert_eq!(parse_term(&print_term(&spec)), Ok(spec.clone()));
    let events: [Event; 3] = [
        mk_created("todo-1", at(8), "ada", "publish the crate", "").expect("valid"),
        mk_spec_revised("todo-1", at(8), "ada", spec, "").expect("valid"),
        mk_completed("todo-1", at(14), "ada", "").expect("valid"),
    ];
    assert_eq!(
        canonical(&events[1]),
        "SpecRevised(todo='todo-1', at=datetime(2026, 9, 8, 9, 0, 0), actor='ada', \
         spec=Decay(start=0.55, end=0.05, end_date=datetime(2026, 9, 15, 9, 0, 0), \
         lead_up=timedelta(days=3), start_date=None), note='')"
    );
    let vocabulary = TodoVocabulary::<Todo>::default();
    for event in &events {
        let back: Event = parse_event(&vocabulary, &canonical(event)).expect("the print parses");
        assert_eq!(&back, event);
    }
}

/// An object that holds a todo's event parses back from its print, and its
/// name is the SHA-256 of that print, as `sha256sum` gives it.
#[test]
fn an_object_of_a_todo_parses_back_and_keeps_its_name() {
    let genesis: Envelope<Event> = Envelope::Genesis(
        mk_genesis("todos", "0123456789abcdef0123456789abcdef").expect("a valid genesis"),
    );
    let event: Event = mk_completed("todo-1", at(14), "ada", "").expect("valid");
    let object =
        Envelope::Change(mk_change(seal_hash(&genesis), Vec::new(), event).expect("no deps"));
    let print = canonical_envelope(&object);
    assert_eq!(
        print,
        "Change(genesis='ae42df129d1f06e1839a70a8a7f66b3c400b7b4bba47defdeb8fdd14b94a23da', \
         deps=(), event=Completed(todo='todo-1', at=datetime(2026, 9, 14, 9, 0, 0), \
         actor='ada', note=''))"
    );
    let back =
        parse_envelope::<Event>(&TodoVocabulary::default(), &print).expect("the print parses");
    assert_eq!(back, object);
    let name = "bd7ab7ec4e78e36f00d2799acf7b6535bd8437e1d86a34063cc890eb87223659";
    assert_eq!(seal_hash(&back), Hash::new(name).expect("a name"));
}
