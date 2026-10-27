use crate::event::Hash;
use crate::laws::Broken;
use crate::schema::Schema;
use crate::store::{sync, Replica};

/// Law 35 between `source` and `fresh`, a replica of the same schema that
/// holds nothing yet: every object of `source` reads back by its name and a
/// name it does not hold reads as absent, a sync into `fresh` takes all of
/// `source`, a second sync takes nothing, and `fresh` then reads as `source`
/// reads: its objects, fold and tips.
///
/// # Errors
///
/// The first of those that does not hold, or a replica that cannot be read.
pub fn synced<E: Schema, R: Replica<E>>(source: &R, fresh: &R) -> Result<(), Broken> {
    let unread = |refused| Broken::of(35, format!("a replica does not read: {refused}"));
    let held = source.held().map_err(unread)?;
    for name in held.dag.objects().keys() {
        if source.print(name).is_none() {
            return Err(Broken::of(
                35,
                format!("{name} is held and does not read back"),
            ));
        }
    }
    let absent = Hash::new("0".repeat(64)).map_err(unread)?;
    if source.print(&absent).is_some() {
        return Err(Broken::of(35, "a name not held reads back"));
    }

    let taken = sync(source, fresh).map_err(unread)?;
    if taken.len() != held.dag.objects().len() {
        return Err(Broken::of(
            35,
            format!(
                "a sync took {} of {} objects",
                taken.len(),
                held.dag.objects().len()
            ),
        ));
    }
    let again = sync(source, fresh).map_err(unread)?;
    if !again.is_empty() {
        return Err(Broken::of(
            35,
            format!("a second sync took {}", again.len()),
        ));
    }

    let copy = fresh.held().map_err(unread)?;
    if copy.dag.objects() != held.dag.objects()
        || copy.folded != held.folded
        || copy.tips != held.tips
    {
        return Err(Broken::of(35, "the copy reads otherwise than its source"));
    }
    Ok(())
}
