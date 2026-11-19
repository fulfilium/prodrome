# Run the script law over your own schema

Law 68 (`SPEC.md` §9) drives three replicas through a random script of
steps and checks every invariant after every step. The machine is
`prodrome::laws::script`, behind the `laws` feature. It is generic over a
schema, so an application can run it over its own schema. This page shows
how. For the design, see [`design-script-law.md`](design-script-law.md).

## Enable the law kit

Add the crate with the `laws` feature to your development dependencies:

```toml
[dev-dependencies]
prodrome-core = { version = "0.9", features = ["laws"] }
proptest = "1"
```

## Write a strategy of writes

The machine needs one thing from you: a function that takes an actor and
returns a `proptest` strategy of the events that the actor writes. Give
each event the actor's name, `who.actor()`, so that the actor's keys sign
it. Draw the instants from a few values, so that writes collide in one
tick and some arrive after readings at later instants:

```rust
use prodrome::laws::script::Who;
use proptest::prelude::*;

fn a_write(who: Who) -> impl Strategy<Value = MyEvent> {
    (0..3usize, 0..4u32).prop_map(move |(entity, minute)| {
        my_event(entity, at_minute(minute), who.actor())
    })
}
```

## Run the law

```rust
use prodrome::laws::script::{self, a_script, an_honest_script};

proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(64))]

    #[test]
    fn my_schema_keeps_the_script_law(script in a_script(a_write, 40)) {
        script::run(&MyVocabulary::default(), &script)
            .map_err(|broken| TestCaseError::fail(broken.to_string()))?;
    }

    #[test]
    fn an_honest_script_verifies_clean(script in an_honest_script(a_write, 40)) {
        script::run(&MyVocabulary::default(), &script)
            .map_err(|broken| TestCaseError::fail(broken.to_string()))?;
    }
}
```

`a_script` draws up to the given number of steps, with faults among the
writes. `an_honest_script` draws no faults. `script::run` returns `Ran`,
with the number of steps and objects, or `Broken`, with the number of the
law that failed, the step after which it failed, and what differs.

## Read a failure

When a script fails, proptest shrinks it and prints the shortest script
that it found, one step on a line, with each event in its canonical print:

```
Script [Memory, Memory, Memory]
  0: Append { by: Ada, of: Ada, writing: Signed, event: ... }
  1: Rotate { by: Cai }
  2: Pick { from: Cai, to: Ben, chosen: [14, 8] }
```

A position such as `chosen` is taken modulo the length of what it picks
from, so the script replays as printed. To keep it as a regression test,
build the same `Script` value in a test and call `script::run`. To replay
a run of proptest itself, set `PROPTEST_RNG_SEED` to the seed of that run.

## Check your own invariants on the same scripts

`script::run` is `script::run_with` with the policy `Everything` and no
check of yours. `script::run_with` takes two more arguments:

- a POLICY (SPEC §5). The machine reads the claimed reading under it, and
  the confirmed reading under it with the root's keys required. Use your
  application's own policy, so that the readings that the machine compares
  are your readings.
- a HOOK, a function that the machine calls before the first step and after
  each step, once for each actor, with the actor (`Who`) and what its
  replica holds (`Held`: the objects, their fold and the tips). The hook
  reads; it cannot change a replica. It returns `Ok(())`, or a `Broken`
  with your own law number and what differs, which stops the script as a
  broken law of the machine does. Proptest then shrinks the script.

`laws::Registers` is the reading that the laws compare: every entity's
registers, each with its frontier and its reading, as object names.
`Registers::of(&held.folded)` reads a replica, and `Registers::differs`
names the first place where two readings part. Use it to compare your own
layer with the replica that it reads.

This example checks, after every step of an honest script, that a cache of
the application's own gives the replica's reading:

```rust
use prodrome::laws::{script, Broken, Registers};
use prodrome::store::Held;

fn my_cache_reads_the_replica(who: Who, held: &Held<MyEvent>) -> Result<(), Broken> {
    let expected = Registers::of(&held.folded);
    let cached = Registers::of(&my_cache_of(held).folded());
    match cached.differs(&expected) {
        None => Ok(()),
        Some(difference) => Err(Broken {
            law: 1001,
            what: format!("{}'s cache: {difference}", who.name()),
        }),
    }
}

proptest! {
    #[test]
    fn my_cache_keeps_up(script in an_honest_script(a_write, 40)) {
        script::run_with(&MyVocabulary::default(), &script, &my_policy(), my_cache_reads_the_replica)
            .map_err(|broken| TestCaseError::fail(broken.to_string()))?;
    }
}
```

## Make the run longer

Raise the number of cases and of steps for a nightly run. `script::long_run`
runs every script that a strategy draws, each by a function you give (for
example `script::run_with` with your policy and hook), and returns a
`Rate`: the scripts and steps it ran, the seconds, and the scripts and
steps per second. A failure is proptest's, with the shortest script:

```rust
let rate = script::long_run(
    &a_script(a_write, 80),
    proptest::test_runner::Config::with_cases(1024),
    |script| script::run(&MyVocabulary::default(), script),
)
.unwrap_or_else(|failure| panic!("{failure}"));
println!("{rate}");
```

The core's own nightly run calls
`script::a_long_run_of_todo_scripts_keeps_every_invariant` with
`--ignored`.
