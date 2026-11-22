mod inner;

use crate::{high, low as _};
use crate::{
    other::Y,
    inner_name,
};
use super::high::X;

/* crate::high in a block comment, /* nested */ crate::high */
// crate::high in a line comment
fn strings() {
    let path = "crate::high // not a comment"; let x = crate::high::X;
    let quote = '"'; let y = crate::other::Y;
    let raw = r#"crate::high "quoted" "#;
    let lifetime: &'static str = "";
    macro_rules! m { () => { $crate::high::X } }
}

#[cfg(test)]
mod tests {
    use crate::high::X;
}

use crate::high::Late;
use prodrome::event::Hash;
