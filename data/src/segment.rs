//! A segment: one step of a path in a nest (design §6.3), and the entity of
//! a qualified FPL reference (SPEC §7.2).

use crate::literal::ProdromeError;

/// One step of a path: an entity's key, or a register's name. A segment is
/// not empty and holds no `/`, so a path prints as its segments joined by
/// `/` and parses back to them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Segment(String);

impl Segment {
    /// # Errors
    ///
    /// An empty segment, or one holding a `/`.
    pub fn new(text: impl Into<String>) -> Result<Segment, ProdromeError> {
        let text = text.into();
        if text.is_empty() || text.contains('/') {
            Err(ProdromeError::invalid(format!(
                "a path segment is not empty and holds no '/', got {text:?}"
            )))
        } else {
            Ok(Segment(text))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
