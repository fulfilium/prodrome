use crate::event::{canonical_envelope, parse_envelope, seal_hash, Hash};
use crate::laws::Broken;
use crate::schema::Schema;

/// Law 1 over `prints`, each an object's name and its print: every print
/// parses at the schema `vocabulary`, prints back byte for byte, and hashes
/// to its name.
///
/// # Errors
///
/// The first print that does not parse, prints back otherwise, or hashes to
/// another name.
pub fn printed<E: Schema>(
    vocabulary: &E::Vocabulary,
    prints: &[(Hash, String)],
) -> Result<(), Broken> {
    for (name, print) in prints {
        let object = parse_envelope::<E>(vocabulary, print)
            .map_err(|refused| Broken::of(1, format!("{name} does not parse: {refused}")))?;
        let again = canonical_envelope(&object);
        if again != *print {
            return Err(Broken::of(1, format!("{name} prints back as {again}")));
        }
        let named = seal_hash(&object);
        if named != *name {
            return Err(Broken::of(1, format!("{name} hashes to {named}")));
        }
    }
    Ok(())
}
