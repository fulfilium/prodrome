//! A long sequence as a balanced tree of chunks, so that a changed element
//! renames only the chunks above it (design §6.2).
//!
//! The shape is a function of the elements alone. A run of nodes closes
//! after a node whose name ends a chunk (one in four, decided by the name's
//! bytes) once the run holds two nodes, or at [`WIDEST`] nodes. The last run
//! of a level closes as it is. So every element is at the same depth, and
//! every chunk holds between two and [`WIDEST`] children, except the last
//! chunk of a level, which can hold one. An appended element renames only
//! the chunks that hold the last element, at most one per level. Two hosts
//! that hold one sequence build one tree with one name, whatever edits each
//! made, so their caches share every entry. An insertion or a deletion
//! moves only the chunk boundaries next to it, up to the next boundary.
//! Inside a run of more than [`WIDEST`] nodes with no boundary, an insertion
//! moves the cuts up to the end of the run.

use super::fold::Tree;
use prodrome_data::event::Hash;

/// The most nodes that one chunk holds. A run with no boundary is cut here,
/// so a run of equal elements still forms a tree.
pub const WIDEST: usize = 16;

/// Whether a run closes after the node named `name`: the name's last hex
/// digit is a multiple of four, so a run is four nodes long on average.
fn ends_a_chunk(name: &Hash) -> bool {
    name.as_str()
        .chars()
        .next_back()
        .and_then(|digit| digit.to_digit(16))
        .is_some_and(|digit| digit % 4 == 0)
}

/// `elements` as one balanced tree, `None` for no elements, and the element
/// itself for one.
///
/// `chunk` builds an interior node from its children in order. It must name
/// the node with [`super::name`] over the children's names and hold their
/// measure: their count, or the sum of any monoid over them. The host's
/// algebra reads a chunk as the monoid sum of its children, such as their
/// concatenation, count or total.
pub fn balance<T: Tree>(elements: Vec<T>, mut chunk: impl FnMut(Vec<T>) -> T) -> Option<T> {
    let mut level = elements;
    while level.len() > 1 {
        let mut runs: Vec<Vec<T>> = Vec::new();
        let mut run = Vec::new();
        for node in level {
            let ends = ends_a_chunk(node.name());
            run.push(node);
            if (ends && run.len() > 1) || run.len() == WIDEST {
                runs.push(std::mem::take(&mut run));
            }
        }
        if !run.is_empty() {
            runs.push(run);
        }
        level = runs.into_iter().map(&mut chunk).collect();
    }
    level.pop()
}
