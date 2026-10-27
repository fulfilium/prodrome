//! Every integration suite of the core, as one test binary: one link instead
//! of one per file, which is most of what compiling the tests cost.

// Beside `all/` rather than in it: the examples include it too.
#[path = "../common/mod.rs"]
mod common;

mod bodies;
mod calibration;
mod causal;
mod chain;
mod change;
mod change_vectors;
mod codec;
mod conformance_view;
mod dag;
mod declared;
mod digest;
mod down_set;
mod events;
mod fold_laws;
mod folds;
mod fpl_laws;
mod fpl_vectors;
mod frontier;
mod fuzz;
mod link_vectors;
mod literals;
mod memo;
mod memo_balance;
mod memo_fold;
mod memory;
mod nest;
mod nest_histories;
mod note;
mod observe;
mod order_laws;
mod recur_vectors;
mod registers;
mod replica;
mod rests_on;
mod review;
mod schedule;
mod schema_laws;
mod series_vectors;
mod sets;
mod signatures;
mod tags;
mod tips;
mod unread;
mod verify_findings;
