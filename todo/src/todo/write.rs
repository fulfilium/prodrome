//! What one event of the todo schema writes.

use prodrome_data::instant::{instant_of, Instant};

use crate::event::{Authored, TodoEvent};
use crate::payload::Payload;
use prodrome_term::fpl::Outcome;
use prodrome_term::term::Term;

/// What one event writes, once for each register that it touches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Write<'e, P> {
    /// A lifecycle write: bound, or `None` for a `Reopened`.
    State(Option<Outcome>),
    Tend(Instant),
    Spec(&'e Term),
    Content(&'e Authored<P>),
}

/// The registers whose frontier can hold a conflict: all but the tendings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    State,
    Spec,
    Content,
}

impl<'e, P: Payload> Write<'e, P> {
    /// The writes of `event`. A record writes its content and, where it
    /// carries a spec, its spec. `Created` writes nothing.
    pub fn of(event: &'e TodoEvent<P>) -> impl Iterator<Item = Write<'e, P>> {
        let at = instant_of(event.at());
        let (first, second) = match event {
            TodoEvent::Created(_) => (None, None),
            TodoEvent::Completed(_) => (Some(Write::State(Some(Outcome::Completed(at)))), None),
            TodoEvent::Cancelled(_) => (Some(Write::State(Some(Outcome::Cancelled(at)))), None),
            TodoEvent::Reopened(_) => (Some(Write::State(None)), None),
            TodoEvent::Tended(_) => (Some(Write::Tend(at)), None),
            TodoEvent::SpecRevised(e) => (Some(Write::Spec(&e.spec)), None),
            TodoEvent::Authored(e) => (Some(Write::Content(e)), e.payload.spec().map(Write::Spec)),
        };
        first.into_iter().chain(second)
    }

    pub fn kind(&self) -> Option<Kind> {
        match self {
            Write::State(_) => Some(Kind::State),
            Write::Spec(_) => Some(Kind::Spec),
            Write::Content(_) => Some(Kind::Content),
            Write::Tend(_) => None,
        }
    }
}

impl Kind {
    /// Every register a todo write supersedes in, in the order a reading
    /// lists them.
    pub const ALL: [Kind; 3] = [Kind::State, Kind::Spec, Kind::Content];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::State => "state",
            Kind::Spec => "spec",
            Kind::Content => "content",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{mk_completed, mk_created};
    use crate::reference::Todo;
    use prodrome_data::literal::Datetime;

    #[test]
    fn a_created_event_writes_no_register() {
        let at = Datetime::new(2026, 9, 6, 0, 0, 0, 0).expect("a real instant");
        let created: TodoEvent<Todo> = mk_created("alpha", at, "bassel", "", "").expect("valid");
        assert_eq!(Write::of(&created).count(), 0);
        let completed: TodoEvent<Todo> = mk_completed("alpha", at, "bassel", "").expect("valid");
        let kinds: Vec<Option<Kind>> = Write::of(&completed).map(|w| w.kind()).collect();
        assert_eq!(kinds, [Some(Kind::State)]);
    }
}
