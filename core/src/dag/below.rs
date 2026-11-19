//! `verify`'s check that no write falls below what it rests on (SPEC §6,
//! law 32), in one pass over each entity's stream.

use crate::dag::Finding;
use crate::fold::Product;
use crate::registers::{BitSet, Folded, Stamp};
use crate::schema::Schema;

/// Every write to an inflationary register that does not stand on the
/// reading of the writes to its entity that it rests on, the empty reading
/// of a first write included: the writes that the append would have
/// refused ([`Product::grows`], law 70).
///
/// An entity whose product has no inflationary register
/// ([`Product::climbing`]) costs one join. Otherwise each write to an
/// inflationary register is checked against the product of the writes to
/// those registers beneath it. That product is the one of its nearest such
/// writes, each kept as it was made, so each write costs a join for each of
/// its nearest writes and not for each write beneath it. In a chain, a write
/// has one nearest write, and the pass is linear in the writes.
pub(super) fn findings<E: Schema>(folded: &Folded<E>) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (_, stream) in folded.entities() {
        let climbing = written(stream);
        if climbing.is_empty() {
            continue;
        }
        let writes: Vec<&Stamp<E>> = stream
            .iter()
            .filter(|stamp| stamp.event.writes().any(|r| climbing.contains(&r)))
            .collect();
        let mut held = BitSet::default();
        for write in &writes {
            held.insert(write.position());
        }
        // Each write's product: of every write beneath it, and itself.
        let mut closed: Vec<E::Registers<'_>> = Vec::with_capacity(writes.len());
        for (at, write) in writes.iter().enumerate() {
            let mut nearest = nearest(&writes, at, &held).into_iter();
            let mut beneath = nearest
                .next()
                .map_or_else(E::Registers::default, |first| closed[first].clone());
            for other in nearest {
                // The reading of each inflationary register is what the
                // check compares, so the reading's writes are enough.
                for register in &climbing {
                    for stamp in closed[other].reading(*register) {
                        beneath.join(stamp);
                    }
                }
            }
            if let Err(why) = beneath.grows(&write.event) {
                findings.push(Finding::Below {
                    write: write.name.clone(),
                    why,
                });
            }
            beneath.join(write);
            closed.push(beneath);
        }
    }
    findings
}

/// The inflationary registers of the schema of `folded`'s writes, read
/// from the product of its first write ([`Product::climbing`]).
pub(super) fn climbing<E: Schema>(folded: &Folded<E>) -> Vec<E::Register> {
    folded
        .entities()
        .next()
        .map_or_else(Vec::new, |(_, stream)| written(stream))
}

/// The inflationary registers of the product of `stream`'s first write.
fn written<E: Schema>(stream: &[Stamp<E>]) -> Vec<E::Register> {
    let mut probe = E::Registers::default();
    if let Some(first) = stream.first() {
        probe.join(first);
    }
    probe.climbing()
}

/// The positions in `writes` of the writes that `writes[at]` rests on and
/// that no other such write rests on, nearest first. `held` holds the
/// writes' positions in their scope. The walk goes back from `at` and stops
/// once the writes found and what they rest on cover every write beneath.
fn nearest<E: Schema>(writes: &[&Stamp<E>], at: usize, held: &BitSet) -> Vec<usize> {
    let write = writes[at];
    let beneath = write.ancestry().count_within(held);
    let mut covered = BitSet::default();
    let mut found = Vec::new();
    let mut count = 0;
    for earlier in (0..at).rev() {
        if count == beneath {
            break;
        }
        let other = writes[earlier];
        if write.descends(other) && !covered.contains(other.position()) {
            found.push(earlier);
            covered.insert(other.position());
            covered.union_with(other.ancestry());
            count = covered.count_within(held);
        }
    }
    found
}
