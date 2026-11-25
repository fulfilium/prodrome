//! Law 68: three replicas driven through a random script of steps, with
//! every invariant checked after every step.
//!
//! A script finds bugs that need several features in sequence, such as a
//! partial sync, a revocation and a read through a cache, which a law of one
//! equation cannot reach. Each of three actors (`ada`, `ben` and `cai`)
//! holds a replica of its own. After every step, [`run`] checks:
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
//! An application runs the machine over its own schema with [`a_script`]
//! and a strategy of the writes of each actor (`docs/script-law.md`). A failure names the law, the step and what
//! differs, and proptest shrinks the script.

mod kept;
mod keys;
mod machine;
mod reading;
mod step;

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use proptest::strategy::Strategy;
use proptest::test_runner::{Config, TestCaseError, TestError, TestRunner};

pub use step::{a_script, an_honest_script, Kind, Refusal, Root, Step, Through, Who, Writing};

use crate::event::canonical;
use crate::laws::Broken;
use crate::policy::{Everything, Policy};
use crate::schema::Schema;
use crate::store::Held;
use machine::{Conduct, Machine};

/// The kind of each actor's replica, by [`Who::index`], and the steps.
/// Its `Debug` output prints one step on a line, with each event in its
/// canonical print.
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

/// The count of steps of a script that kept every invariant, and the count
/// of objects that its replicas held together at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ran {
    pub steps: usize,
    pub objects: usize,
}

/// [`run_with`] under [`Everything`], with no check of the host.
///
/// # Errors
///
/// The first invariant that does not hold: the law it states, the step
/// after which it failed, and what differs.
pub fn run<E: Schema>(vocabulary: &E::Vocabulary, script: &Script<E>) -> Result<Ran, Broken> {
    run_with(vocabulary, script, &Everything, |_, _| Ok(()))
}

/// Runs `script` over replicas opened at `vocabulary`, with the claimed
/// reading under `policy`, and checks every invariant after every step.
///
/// Before the first step and after each step, after the checks of the
/// machine, `run_with` calls `host` with each actor and the contents of its
/// replica, read-only. A refusal by `host` stops the script as a broken law
/// does.
///
/// # Errors
///
/// The first invariant that does not hold, the machine's or the host's: the
/// law it states, the step after which it failed, and what differs.
pub fn run_with<E, P>(
    vocabulary: &E::Vocabulary,
    script: &Script<E>,
    policy: &P,
    mut host: impl FnMut(Who, &Held<E>) -> Result<(), Broken>,
) -> Result<Ran, Broken>
where
    E: Schema,
    P: Policy<E> + Clone,
{
    let mut machine = Machine::new(
        vocabulary,
        script.kinds,
        Conduct::of(&script.steps),
        policy.clone(),
    )?;
    machine
        .check(&mut host)
        .map_err(|broken| at_step(broken, "before the first step"))?;
    for (n, step) in script.steps.iter().enumerate() {
        machine
            .step(n, step)
            .and_then(|()| machine.check(&mut host))
            .map_err(|broken| at_step(broken, &format!("at step {n}")))?;
    }
    Ok(Ran {
        steps: script.steps.len(),
        objects: machine.objects()?,
    })
}

/// The counts of scripts and steps of a long run, and its duration in
/// seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rate {
    pub scripts: usize,
    pub steps: usize,
    pub seconds: f64,
}

impl Rate {
    /// Scripts run in each second.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn scripts_per_second(&self) -> f64 {
        self.scripts as f64 / self.seconds
    }

    /// Steps run in each second.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn steps_per_second(&self) -> f64 {
        self.steps as f64 / self.seconds
    }
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} scripts, {} steps, in {:.1} s: {:.2} scripts and {:.1} steps per second",
            self.scripts,
            self.steps,
            self.seconds,
            self.scripts_per_second(),
            self.steps_per_second()
        )
    }
}

/// Runs every script that `scripts` draws under `config` with `run` (for
/// example [`run`], or [`run_with`] with the policy and check of a host),
/// and returns the rate.
///
/// # Errors
///
/// The shortest failing script that proptest found, with what broke.
pub fn long_run<E: Schema>(
    scripts: &impl Strategy<Value = Script<E>>,
    config: Config,
    run: impl Fn(&Script<E>) -> Result<Ran, Broken>,
) -> Result<Rate, TestError<Script<E>>> {
    let mut runner = TestRunner::new(config);
    let (count, steps) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let started = Instant::now();
    runner.run(scripts, |script| {
        let ran = run(&script).map_err(|broken| TestCaseError::fail(broken.to_string()))?;
        steps.fetch_add(ran.steps, Ordering::Relaxed);
        count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    })?;
    Ok(Rate {
        scripts: count.into_inner(),
        steps: steps.into_inner(),
        seconds: started.elapsed().as_secs_f64(),
    })
}

fn at_step(Broken { law, what }: Broken, when: &str) -> Broken {
    Broken {
        law,
        what: format!("{when}: {what}"),
    }
}
