//! FPL, the term language, as data (SPEC §7). This module evaluates nothing.
//!
//! The `mk_*` smart constructors check the invariants of a term, and
//! `parse_term` calls them. `Term::new` skips the checks.

mod construct;
mod delta;
mod env;
mod printed;

pub use construct::{
    checklist, least_of, mk_absent, mk_after, mk_conj, mk_curve, mk_decay, mk_flat, mk_gate,
    mk_importance, mk_least, mk_offset, mk_offset_by, mk_periodic, mk_piecewise, mk_recur, mk_ref,
    mk_ref_in, mk_shift, mk_within, piecewise,
};
pub use delta::{delta_from_hours, div_delta, hours, total_seconds, us_of, Delta};
pub use env::{Candidates, Env, Outcome};
pub use printed::{parse_term, print_term};
pub use prodrome_data::instant::{datetime_of, instant_of, iso, parse_iso, FplError, Instant};

/// The conjunction exponent of a checklist, and of a `Conj` whose literal
/// leaves out `p`.
pub const PRIORITY_POWER: f64 = -4.0;

fn err<T>(msg: impl Into<String>) -> Result<T, FplError> {
    Err(FplError(msg.into()))
}
