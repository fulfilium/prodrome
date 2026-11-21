//! The steps of a placement at which the unit tests inject a fault: the
//! staging of one print, the sync of the batch, and the sync of the
//! directory. Outside the unit tests, no step fails here.

use crate::literal::ProdromeError;

/// A step of [`super::EventStore::place`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    /// The staging of the print at this place in the batch.
    Stage(usize),
    /// The sync of every staged byte.
    Sync,
    /// The sync of `objects/` after the renames.
    SyncDir,
}

#[cfg(test)]
thread_local! {
    static AT: std::cell::Cell<Option<Step>> = const { std::cell::Cell::new(None) };
}

/// The fault injected at `step`, if a unit test injected one there.
///
/// # Errors
///
/// In a unit test, where the test injected a fault at `step`.
pub(super) fn at(step: Step) -> Result<(), ProdromeError> {
    #[cfg(test)]
    if AT.with(std::cell::Cell::get) == Some(step) {
        return Err(ProdromeError::Store(format!(
            "a fault injected at {step:?}"
        )));
    }
    let _ = step;
    Ok(())
}

/// A fault at `step` for the placements of this thread, or none.
#[cfg(test)]
pub(super) fn inject(step: Option<Step>) {
    AT.with(|at| at.set(step));
}
