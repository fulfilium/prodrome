//! The registers of a todo, as a product of the register algebra.

use prodrome_data::event::TodoId;
use prodrome_data::fold::{Frontier, GrowSet, Product, Register};
use prodrome_data::instant::Instant;
use prodrome_data::literal::ProdromeError;
use prodrome_data::stamp::Stamp;

use super::{Content, Kind, Spec, State, Write};
use crate::event::{Authored, TodoEvent};
use crate::payload::Payload;
use prodrome_term::fpl::{Candidates, Env};
use prodrome_term::term::Term;

/// One todo's registers at a moment (SPEC §6, "The todo schema's
/// registers").
#[derive(Debug, Clone, PartialEq)]
pub struct Registers<'a, P: Payload> {
    pub state: Frontier<'a, TodoEvent<P>>,
    pub spec: Frontier<'a, TodoEvent<P>>,
    pub content: Frontier<'a, TodoEvent<P>>,
    pub tended: GrowSet<Instant>,
}

impl<P: Payload> Default for Registers<'_, P> {
    fn default() -> Self {
        Registers {
            state: Frontier::default(),
            spec: Frontier::default(),
            content: Frontier::default(),
            tended: GrowSet::new(),
        }
    }
}

impl<'a, P: Payload> Product<'a> for Registers<'a, P> {
    type Schema = TodoEvent<P>;

    fn registers(&self) -> Vec<Kind> {
        Kind::ALL.to_vec()
    }

    fn join(&mut self, stamp: &'a Stamp<TodoEvent<P>>) {
        for write in Write::of(&stamp.event) {
            self.route(stamp, write);
        }
    }

    fn frontier(&self, kind: Kind) -> &Frontier<'a, TodoEvent<P>> {
        match kind {
            Kind::State => &self.state,
            Kind::Spec => &self.spec,
            Kind::Content => &self.content,
        }
    }

    fn join_to(&mut self, kind: Kind, stamp: &'a Stamp<TodoEvent<P>>) {
        match kind {
            Kind::State => self.state.join(stamp),
            Kind::Spec => self.spec.join(stamp),
            Kind::Content => self.content.join(stamp),
        }
    }

    fn reading(&self, kind: Kind) -> Vec<&'a Stamp<TodoEvent<P>>> {
        match kind {
            Kind::State => self.state.reading::<State>(),
            Kind::Spec => self.spec.reading::<Spec>(),
            Kind::Content => self.content.reading::<Content>(),
        }
    }

    /// No todo register is inflationary: a `Reopened` after a `Completed`
    /// goes back, which is why a todo's conflicts are shown and priced.
    fn grows(&self, _event: &TodoEvent<P>) -> Result<(), ProdromeError> {
        Ok(())
    }

    /// No todo register is inflationary, so no reading of one must climb.
    fn climbed(&self, _earlier: &Self) -> Result<(), ProdromeError> {
        Ok(())
    }

    fn climbing(&self) -> Vec<Kind> {
        Vec::new()
    }
}

impl<'a, P: Payload> Registers<'a, P> {
    fn route(&mut self, stamp: &'a Stamp<TodoEvent<P>>, write: Write<'a, P>) {
        match write {
            Write::State(_) => self.state.join(stamp),
            Write::Spec(_) => self.spec.join(stamp),
            Write::Content(_) => self.content.join(stamp),
            Write::Tend(at) => self.tended.join(at),
        }
    }

    /// The candidate bindings, `{None}` for an open todo.
    pub fn outcomes(&self) -> Candidates {
        let mut out: Candidates = self
            .state
            .reading::<State>()
            .into_iter()
            .flat_map(|stamp| Write::of(&stamp.event))
            .filter_map(|write| match write {
                Write::State(binding) => Some(binding),
                _ => None,
            })
            .collect();
        if out.is_empty() {
            out.insert(None);
        }
        out
    }

    pub fn specs(&self) -> Vec<&'a Term> {
        self.spec
            .reading::<Spec>()
            .into_iter()
            .flat_map(|stamp| Write::of(&stamp.event))
            .filter_map(|write| match write {
                Write::Spec(spec) => Some(spec),
                _ => None,
            })
            .collect()
    }

    pub fn records(&self) -> Vec<&'a Authored<P>> {
        self.content
            .reading::<Content>()
            .into_iter()
            .flat_map(|stamp| Write::of(&stamp.event))
            .filter_map(|write| match write {
                Write::Content(record) => Some(record),
                _ => None,
            })
            .collect()
    }

    /// Records this todo in `env`: its candidate bindings unless it is simply
    /// open, and its tendings.
    pub fn bind(&self, todo: &TodoId, env: &mut Env) {
        let outcomes = self.outcomes();
        if outcomes != Candidates::from([None]) {
            env.outcomes.insert(todo.as_str().to_owned(), outcomes);
        }
        if !self.tended.is_empty() {
            env.tended
                .insert(todo.as_str().to_owned(), self.tended.clone());
        }
    }
}
