//! What the laws compare: a fold's registers by object name, and a replica's
//! objects, tips and registers. A difference names the first place the two
//! sides part, with both sides.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use crate::dag::Dag;
use crate::event::Hash;
use crate::fold::{read, Product};
use crate::policy::Everything;
use crate::registers::{Folded, Genesis};
use crate::schema::Schema;

/// One register: its name, its frontier and its reading, as object names.
type Register<E> = (<E as Schema>::Register, Vec<Hash>, Vec<Hash>);

/// Every entity's registers, by genesis and key.
pub(super) struct Registers<E: Schema>(BTreeMap<(Genesis, E::Key), Vec<Register<E>>>);

impl<E: Schema> Registers<E> {
    /// The registers of every entity `folded` holds, each read whole.
    pub(super) fn of(folded: &Folded<E>) -> Self {
        let mut out = BTreeMap::new();
        for (genesis, prodrome) in folded.prodromes() {
            for (key, stream) in prodrome {
                let registers = read(stream, None, &Everything);
                let each = registers
                    .registers()
                    .into_iter()
                    .map(|register| {
                        let names = registers
                            .reading(register)
                            .iter()
                            .map(|stamp| stamp.name.clone())
                            .collect();
                        (register, registers.frontier(register).names(), names)
                    })
                    .collect();
                out.insert((genesis.clone(), key.clone()), each);
            }
        }
        Registers(out)
    }

    /// The first entity or register where `self` and `expected` part, with
    /// both sides; `None` where they agree.
    pub(super) fn differs(&self, expected: &Self) -> Option<String> {
        let entities: BTreeSet<&(Genesis, E::Key)> =
            self.0.keys().chain(expected.0.keys()).collect();
        for entity in entities {
            match (self.0.get(entity), expected.0.get(entity)) {
                (Some(got), Some(want)) if got == want => {}
                (Some(got), Some(want)) => {
                    let (got, want) = got
                        .iter()
                        .zip(want)
                        .find(|(got, want)| got != want)
                        .map_or((got.as_slice(), want.as_slice()), |(got, want)| {
                            (std::slice::from_ref(got), std::slice::from_ref(want))
                        });
                    return Some(format!(
                        "entity {:?}: (register, frontier, reading) {got:?}, expected {want:?}",
                        entity.1
                    ));
                }
                (got, want) => {
                    return Some(format!(
                        "entity {:?}: {}, expected {}",
                        entity.1,
                        held(got.is_some()),
                        held(want.is_some())
                    ));
                }
            }
        }
        None
    }
}

fn held(is: bool) -> &'static str {
    if is {
        "held"
    } else {
        "absent"
    }
}

/// What a replica reads: its objects, its tips and its registers.
pub(super) struct Replicated<E: Schema> {
    objects: BTreeSet<Hash>,
    tips: BTreeSet<Hash>,
    registers: Registers<E>,
}

impl<E: Schema> Replicated<E> {
    /// What a fresh replica holding `dag`'s objects reads.
    pub(super) fn of(dag: &Dag<E>, folded: &Folded<E>) -> Self {
        Replicated {
            objects: dag.objects().keys().cloned().collect(),
            tips: dag.tips(),
            registers: Registers::of(folded),
        }
    }

    /// The first place where `self` and `expected` part, with both sides.
    pub(super) fn differs(&self, expected: &Self) -> Option<String> {
        if self.objects != expected.objects {
            return Some(sets("objects", &self.objects, &expected.objects));
        }
        if self.tips != expected.tips {
            return Some(sets("tips", &self.tips, &expected.tips));
        }
        self.registers.differs(&expected.registers)
    }
}

/// Two sets of names, by what each holds that the other does not.
fn sets<T: Ord + Debug>(what: &str, got: &BTreeSet<T>, want: &BTreeSet<T>) -> String {
    let extra: Vec<&T> = got.difference(want).collect();
    let missing: Vec<&T> = want.difference(got).collect();
    format!("{what}: {extra:?} not expected, {missing:?} missing")
}
