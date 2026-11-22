//! A program whose size in WebAssembly shows what the code of `Dag` costs
//! for each schema that a program uses it at. It instantiates `Dag` at
//! three schemas and calls `extend`, `clone`, `tips` and `linearise` on
//! each. Build it BY HAND, from this directory (`cargo build --release
//! --target wasm32-unknown-unknown`), and compare the size of
//! `target/wasm32-unknown-unknown/release/dag-sizes.wasm` across commits.
//! Running it does nothing useful.

#[path = "../../../../src/data/schema/headed/example.rs"]
#[allow(dead_code)]
mod note;

use std::hint::black_box;

use prodrome::dag::Dag;
use prodrome::declared::Declared;
use prodrome::event::{Envelope, Hash, TodoEvent};
use prodrome::reference::Todo;
use prodrome::schema::Schema;

/// `extend`, `clone`, `tips` and `linearise` of a DAG at the schema `E`.
fn exercise<E: Schema>(objects: Vec<(Hash, Envelope<E>)>) -> usize {
    let mut dag: Dag<E> = Dag::from_iter([]);
    dag.extend(objects);
    let copy = dag.clone();
    copy.tips().len() + dag.linearise().map_or(0, |order| order.len())
}

fn main() {
    let total = exercise::<TodoEvent<Todo>>(black_box(Vec::new()))
        + exercise::<note::Note>(black_box(Vec::new()))
        + exercise::<Declared>(black_box(Vec::new()));
    black_box(total);
}
