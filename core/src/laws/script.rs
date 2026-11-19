//! Law 68, the script law: three replicas driven through a random script of
//! steps, with every invariant checked after every step.
//!
//! A law of one statement draws its inputs and checks one equation. Bugs
//! that need several features in sequence (a partial sync, then a
//! revocation, then a read through a cache) are out of its reach. A script
//! reaches them: each of three actors (`ada`, `ben` and `cai`) holds a
//! replica of its own, and a [`Script`] is a list of [`Step`]s, each one an
//! append, a partial or whole sync, a refused receipt, a revocation of a
//! key, a read, a recorded reading, a snapshot, a forget or a flush. After
//! every step, [`run`] checks:
//!
//! - each replica's kept fold is the fold of what it holds, and its tips
//!   and heads are its history's;
//! - what each replica answered it took, applied in order to a shadow
//!   replica, is what it holds and folds as it does;
//! - `verify` finds exactly the faults the script wrote, and the objects
//!   that no key proves by the script's own account of keys; nothing at all
//!   in an honest script;
//! - replicas that hold the same objects read alike, claimed and confirmed;
//! - every reading recorded at a down-set reads the same in every replica
//!   that holds the down-set, however it grew;
//! - every reading the cache holds is the fresh reading of every replica
//!   whose frontier its key names;
//! - no reading of an inflationary register falls in any replica, though a
//!   fault writes a value below the reading (law 32);
//! - and, at the step itself, that an append or a receipt done twice changes
//!   nothing, that a refused receipt takes nothing, and that a sync of all
//!   leaves every replica holding the same objects.
//!
//! The machine is generic over a schema. An application runs it over its
//! own schema with [`a_script`] and a strategy of the writes each actor
//! makes; `docs/script-law.md` shows how. A failure names the law, the step
//! and what differs, and proptest shrinks the script to a short one, which
//! [`Script`]'s `Debug` prints one step on a line with each event in its
//! canonical print.

mod kept;
mod keys;
mod machine;
mod reading;
mod step;

use std::fmt;

pub use step::{a_script, an_honest_script, Kind, Refusal, Root, Step, Through, Who, Writing};

use crate::event::canonical;
use crate::laws::Broken;
use crate::schema::Schema;
use machine::{Conduct, Machine};

/// A script: the kind of each actor's replica, by [`Who::index`], and the
/// steps.
#[derive(Clone, PartialEq)]
pub struct Script<E> {
    pub kinds: [Kind; 3],
    pub steps: Vec<Step<E>>,
}

/// One step on a line, each event in its canonical print.
impl<E: Schema> fmt::Debug for Script<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Script {:?}", self.kinds)?;
        for (n, step) in self.steps.iter().enumerate() {
            match step {
                Step::Append {
                    by,
                    of,
                    event,
                    writing,
                } => writeln!(
                    f,
                    "  {n}: Append {{ by: {by:?}, of: {of:?}, writing: {writing:?}, event: {} }}",
                    canonical(event)
                )?,
                Step::Pick { .. }
                | Step::Sync { .. }
                | Step::SyncAll
                | Step::Refuse { .. }
                | Step::Rotate { .. }
                | Step::Revoke { .. }
                | Step::Read { .. }
                | Step::Record { .. }
                | Step::Snapshot { .. }
                | Step::Forget { .. }
                | Step::Flush { .. } => writeln!(f, "  {n}: {step:?}")?,
            }
        }
        Ok(())
    }
}

/// What a script that kept every invariant did: its steps, and the objects
/// its replicas held together at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ran {
    pub steps: usize,
    pub objects: usize,
}

/// Run `script` over replicas opened at the schema `vocabulary`, and check
/// every invariant after every step.
///
/// # Errors
///
/// The first invariant that does not hold: the law it states, the step
/// after which it failed, and what differs.
pub fn run<E: Schema>(vocabulary: &E::Vocabulary, script: &Script<E>) -> Result<Ran, Broken> {
    let mut machine = Machine::new(vocabulary, script.kinds, Conduct::of(&script.steps))?;
    machine
        .check()
        .map_err(|broken| at_step(broken, "before the first step"))?;
    for (n, step) in script.steps.iter().enumerate() {
        machine
            .step(n, step)
            .and_then(|()| machine.check())
            .map_err(|broken| at_step(broken, &format!("at step {n}")))?;
    }
    Ok(Ran {
        steps: script.steps.len(),
        objects: machine.objects()?,
    })
}

fn at_step(Broken { law, what }: Broken, when: &str) -> Broken {
    Broken {
        law,
        what: format!("{when}: {what}"),
    }
}
