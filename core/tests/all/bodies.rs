//! `fold::sole::Bodies`, the register type whose value is an event's body
//! under the body's own order, over a FLAG schema in the tests only: a flag
//! is set off or on, `Off < On`, and a reader can record that they viewed
//! it, which writes no register. Law 31 (the reading is the maximal values)
//! and law 33 run over it.

use prodrome::event::Actor;
use prodrome::fold::sole::{Bodies, One, Only};
use prodrome::fold::{read, Order, Product};
use prodrome::literal::codec::{Constructor, Fields};
use prodrome::literal::{Datetime, ProdromeError, Value};
use prodrome::policy::Everything;
use prodrome::registers::{fold, Stamp};
use prodrome::schema::headed::{Body, Event};
use prodrome::schema::Schema;
use proptest::prelude::*;

use crate::common::draw::{a_draw, history};
use crate::schema_laws::{free, routed};

const SET_OFF: Constructor = ("SetOff", &["flag", "at", "actor"]);
const SET_ON: Constructor = ("SetOn", &["flag", "at", "actor"]);
const VIEWED: Constructor = ("Viewed", &["flag", "at", "actor"]);

/// What an event says of its flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flag {
    Off,
    On,
    Viewed,
}

/// `Off < On`. A view writes no register, so the register never orders it.
impl Order for Flag {
    fn le(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Flag::Off, Flag::Off | Flag::On) | (Flag::On, Flag::On) | (Flag::Viewed, Flag::Viewed)
        )
    }
}

type FlagEvent = Event<Flag>;

impl Body for Flag {
    type Key = String;
    type Register = Only;
    type Registers<'a> = One<'a, Bodies<Flag>>;
    type Refusal = std::convert::Infallible;

    const EVENTS: &'static [Constructor] = &[SET_OFF, SET_ON, VIEWED];

    fn writes(&self) -> impl Iterator<Item = Only> {
        match self {
            Flag::Off | Flag::On => Some(Only),
            Flag::Viewed => None,
        }
        .into_iter()
    }

    fn fields(&self) -> (Constructor, Vec<Value>) {
        let constructor = match self {
            Flag::Off => SET_OFF,
            Flag::On => SET_ON,
            Flag::Viewed => VIEWED,
        };
        (constructor, Vec::new())
    }

    fn parse(fields: &Fields<'_>) -> Result<FlagEvent, ProdromeError> {
        let flag = match fields.name() {
            "SetOff" => Flag::Off,
            "SetOn" => Flag::On,
            "Viewed" => Flag::Viewed,
            other => return Err(prodrome::literal::codec::no_arm("a flag event", other)),
        };
        Ok(Event::of(
            fields.get("flag")?,
            fields.get("at")?,
            fields.get("actor")?,
            flag,
        ))
    }
}

fn a_flag_event() -> impl Strategy<Value = FlagEvent> {
    (
        0..2usize,
        0..3u32,
        prop::sample::select(vec![Flag::Off, Flag::On, Flag::Viewed]),
    )
        .prop_map(|(name, minute, flag)| {
            let at = Datetime::new(2026, 10, 1, 0, minute, 0, 0).expect("a real instant");
            let ada = Actor::new("ada").expect("a valid actor");
            Event::of(["f1", "f2"][name].to_owned(), at, ada, flag)
        })
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    /// Law 31 under `Bodies`: the register reads the greatest body among
    /// the writes that no other write descends from, named by the least
    /// write that carries it; a view is never among them.
    #[test]
    fn a_flag_reads_its_greatest_body(draw in a_draw(a_flag_event())) {
        let dag: prodrome::dag::Dag<FlagEvent> = history(&draw.events).into_iter().collect();
        let folded = fold(&dag.nodes().expect("a DAG"));
        for (key, stream) in folded.entities() {
            let writes: Vec<&Stamp<FlagEvent>> = stream
                .iter()
                .filter(|stamp| stamp.event.writes().next().is_some())
                .collect();
            let mut frontier: Vec<&Stamp<FlagEvent>> = writes
                .iter()
                .copied()
                .filter(|write| !writes.iter().any(|later| later.descends(write)))
                .collect();
            frontier.sort_by(|a, b| a.name.cmp(&b.name));
            let greatest = frontier
                .iter()
                .find(|stamp| *stamp.event.body() == Flag::On)
                .or_else(|| frontier.first())
                .map(|stamp| stamp.name.clone());
            let registers: One<'_, Bodies<Flag>> = read(stream, None, &Everything);
            let reading: Vec<_> = registers.reading(Only).iter().map(|s| s.name.clone()).collect();
            prop_assert_eq!(reading, greatest.into_iter().collect::<Vec<_>>(), "{:?}", key);
        }
    }

    /// Law 33 under `Bodies`: a reading is a function of the object set.
    #[test]
    fn a_flag_reading_is_a_function_of_the_object_set(draw in a_draw(a_flag_event())) {
        free(&draw)?;
    }

    /// Law 33 under `Bodies`: a write joins the registers it names, and the
    /// type values exactly those writes.
    #[test]
    fn a_flag_write_joins_the_registers_it_names(event in a_flag_event()) {
        routed(&event)?;
        prop_assert_eq!(
            <Bodies<Flag> as prodrome::fold::RegisterType<FlagEvent>>::value(&event).is_some(),
            event.writes().next().is_some()
        );
    }
}
