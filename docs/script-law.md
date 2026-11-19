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

## Make the run longer

Raise the number of cases and of steps for a nightly run. The core's own
nightly run calls `script::a_long_run_of_todo_scripts_keeps_every_invariant`
with `--ignored`.
