use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::dag::Dag;
use crate::event::{canonical_envelope, Hash};
use crate::laws::reading::Replicated;
use crate::laws::{printed, Broken};
use crate::literal::ProdromeError;
use crate::registers::fold;
use crate::schema::Schema;
use crate::store::{sync, Held, Replica};

/// A replica whose wire is cut: after `left` prints, it gives none.
struct Cut<'r, E, R: ?Sized> {
    replica: &'r R,
    left: AtomicUsize,
    schema: PhantomData<fn() -> E>,
}

impl<E: Schema, R: Replica<E> + ?Sized> Replica<E> for Cut<'_, E, R> {
    fn schema(&self) -> &E::Vocabulary {
        self.replica.schema()
    }

    fn held(&self) -> Result<Held<E>, ProdromeError> {
        self.replica.held()
    }

    fn tips(&self) -> Result<BTreeSet<Hash>, ProdromeError> {
        self.replica.tips()
    }

    fn print(&self, name: &Hash) -> Option<Vec<u8>> {
        let left = self.left.load(Ordering::Relaxed);
        self.left.store(left.saturating_sub(1), Ordering::Relaxed);
        (left > 0).then(|| self.replica.print(name)).flatten()
    }

    fn append(&self, event: E) -> Result<Hash, ProdromeError> {
        self.replica.append(event)
    }

    fn receive(
        &self,
        seeds: BTreeSet<Hash>,
        print_of: &dyn Fn(&Hash) -> Option<Vec<u8>>,
    ) -> Result<Vec<Hash>, ProdromeError> {
        self.replica.receive(seeds, print_of)
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "`map_err` hands the error over by value"
)]
fn unread(refused: ProdromeError) -> Broken {
    Broken::of(35, format!("a replica does not read: {refused}"))
}

/// What `replica` reads.
fn observed<E: Schema>(replica: &(impl Replica<E> + ?Sized)) -> Result<Replicated<E>, Broken> {
    let Held { dag, folded, .. } = replica.held().map_err(unread)?;
    Ok(Replicated::of(&dag, &folded))
}

/// Law 1 at every object `replica` holds: it reads back by its name as its
/// print, which parses, prints back and hashes to the name.
fn read_back<E: Schema>(replica: &(impl Replica<E> + ?Sized)) -> Result<(), Broken> {
    let held = replica.held().map_err(unread)?;
    let mut prints = Vec::new();
    for (name, object) in held.dag.objects() {
        let print = replica
            .print(name)
            .ok_or_else(|| Broken::of(1, format!("{name} is held and does not read back")))?;
        let print = String::from_utf8(print)
            .map_err(|_| Broken::of(1, format!("{name} reads back as no text")))?;
        if print != canonical_envelope(object) {
            return Err(Broken::of(1, format!("{name} reads back as {print}")));
        }
        prints.push((name.clone(), print));
    }
    printed::<E>(replica.schema(), &prints)
}

/// The join clauses of law 35 between `here` and `there`, replicas of one
/// schema held anywhere, with `empty` making replicas that hold nothing:
///
/// - a sync from `there` into a copy of `here` over a wire cut after `cut`
///   prints takes nothing where it fails;
/// - a sync of the down-set of one object of `there` (its `partial`-th by
///   name, modulo how many it holds), then a whole sync, leaves the copy
///   reading as the union of both replicas reads: the fold of the union of
///   their objects, with its tips;
/// - a second sync takes nothing;
/// - `here` and `there` synced into a third replica in either order read
///   the same.
///
/// Before them, law 1 at every object either holds: each reads back by its
/// name as its print.
///
/// The kit does not check §3's rules on what a change rests on, nor law
/// 35's clauses on appends in memory and on overlays. A host runs
/// [`Dag::verify`] over its stores for §3's rules.
///
/// # Errors
///
/// The first clause that does not hold, naming the first difference with
/// both sides, or a replica that cannot be read.
pub fn synced<E, H, T, F>(
    here: &H,
    there: &T,
    empty: impl Fn() -> F,
    cut: usize,
    partial: usize,
) -> Result<(), Broken>
where
    E: Schema,
    H: Replica<E>,
    T: Replica<E>,
    F: Replica<E>,
{
    read_back(here)?;
    read_back(there)?;
    let union = {
        let (mine, theirs) = (here.held().map_err(unread)?, there.held().map_err(unread)?);
        let dag: Dag<E> = mine
            .dag
            .objects()
            .iter()
            .chain(theirs.dag.objects())
            .map(|(name, object)| (name.clone(), object.clone()))
            .collect();
        let nodes = dag.nodes().map_err(unread)?;
        Replicated::of(&dag, &fold(&nodes))
    };
    let reads_as_union = |replica: &F, how: &str| -> Result<(), Broken> {
        match observed(replica)?.differs(&union) {
            Some(difference) => Err(Broken::of(35, format!("{how}: {difference}"))),
            None => Ok(()),
        }
    };

    let copy = empty();
    sync(here, &copy).map_err(unread)?;
    let before = observed(&copy)?;
    let wire = Cut {
        replica: there,
        left: AtomicUsize::new(cut),
        schema: PhantomData,
    };
    if sync(&wire, &copy).is_err() {
        if let Some(difference) = observed(&copy)?.differs(&before) {
            return Err(Broken::of(
                35,
                format!("a cut sync took something: {difference}"),
            ));
        }
    }

    let theirs: Vec<Hash> = there
        .held()
        .map_err(unread)?
        .dag
        .objects()
        .keys()
        .cloned()
        .collect();
    if !theirs.is_empty() {
        let seed = theirs[partial % theirs.len()].clone();
        copy.receive([seed].into(), &|name| there.print(name))
            .map_err(unread)?;
    }
    sync(there, &copy).map_err(unread)?;
    reads_as_union(&copy, "a down-set, then the whole")?;
    let again = sync(there, &copy).map_err(unread)?;
    if !again.is_empty() {
        return Err(Broken::of(35, format!("a second sync took {again:?}")));
    }
    reads_as_union(&copy, "after a second sync")?;

    let one = empty();
    sync(here, &one).map_err(unread)?;
    sync(there, &one).map_err(unread)?;
    reads_as_union(&one, "here, then there, into a third")?;
    let other = empty();
    sync(there, &other).map_err(unread)?;
    sync(here, &other).map_err(unread)?;
    reads_as_union(&other, "there, then here, into a third")
}
