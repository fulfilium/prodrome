//! The laws of a store of record, run from one suite over a factory of
//! stores (`docs/design-table-store.md`, stage 1).
//!
//! A store of record is a store that holds the lock that every writer
//! honours, so that it alone can make a decision (SPEC §3). Each law here is
//! a function of one store, and each test runs it over every store that
//! [`kind::stores_of_record`] makes, or over every replica that
//! [`kind::replicas`] makes. The tests state each step that moves a file as
//! an operation that every store of record has: a print placed, a print
//! replaced, a print set aside, and a stop after each step of a write
//! ([`kind::StoreOfRecord`]).
//!
//! The laws, and the stores that each one runs over:
//!
//! | Law | Test | Stores of record | `MemoryStore` |
//! |---|---|---|---|
//! | 40 | `reading.rs` | yes | no: a receipt refuses an object whose parent the store lacks, and nothing takes an object away, so a store in memory always holds a down-set |
//! | 45 | `reading.rs`, `script.rs` | yes | no: a receipt verifies every print, so a store in memory holds no unread print, and it has no second handle to read cold |
//! | 60, 62, 63 | `script.rs` | yes | no: a store in memory keeps no record of heads; its `heads()` is the provided method, which reads them from its history |
//! | 61 | `stops.rs` | yes | no: a stop of a process leaves nothing of a store in memory |
//! | 65 | `script.rs` | yes | no: as for 60 |
//! | 72 | `husks.rs` | yes | no: a store in memory has no `husk` |
//! | 73, 74, 77 | `husks.rs` | yes | yes |
//! | 75 | `stops.rs` | yes | no: a store in memory replaces no print |
//! | 76 | `husks.rs` | yes | no: a store in memory has no run of appends under one hold |
//! | 79 | `stops.rs` | yes | no: a placement in memory does not fail |
//!
//! Law 64 reads the positions of a held `Dag`, and no store's prints, so it
//! stays in `frontier_reach.rs`. Law 68's script runs its own three replicas
//! (`script.rs` at the top of the test tree), and stays.

mod husks;
mod kind;
mod reading;
mod script;
mod stops;

pub(crate) use kind::{begin, copy_prints, directory, Scratch, StoreOfRecord, WithDoors};
