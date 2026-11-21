//! Whether git tracks a file of a store: a read of the index of the git
//! repository that holds the store's root.
//!
//! The record of heads (`.heads`) and the count of holds (`.writes`) are
//! local to one machine. A repository that tracks one of them carries one
//! machine's record into every clone, so `verify` reports it. The store
//! reads the index itself, in versions 2, 3 and 4 of its format, and runs
//! no program.

use std::fs;
use std::path::{Path, PathBuf};

use crate::literal::ProdromeError;

/// Does the index of the git repository that holds `root` list
/// `root/<file>`? No where `root` is in no repository, or the repository
/// has no index.
///
/// # Errors
///
/// The index, or the `.git` file that names it, does not read or does not
/// parse.
pub(super) fn tracked(root: &Path, file: &str) -> Result<bool, ProdromeError> {
    let Ok(root) = fs::canonicalize(root) else {
        return Ok(false);
    };
    let Some((tree, git)) = repository(&root)? else {
        return Ok(false);
    };
    let index = git.join("index");
    let bytes = match fs::read(&index) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(ProdromeError::io(&index, &error)),
    };
    let within = root.strip_prefix(&tree).unwrap_or(Path::new(""));
    let path = within
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .chain([file.to_owned()])
        .collect::<Vec<_>>()
        .join("/");
    let width = id_width(&git);
    Ok(paths(&bytes, width)
        .map_err(|why| ProdromeError::Store(format!("{} does not parse: {why}", index.display())))?
        .iter()
        .any(|listed| listed.as_bytes() == path.as_bytes()))
}

/// The work tree and the git directory of the repository that holds `dir`:
/// the nearest ancestor with a `.git` directory, or a `.git` file that
/// names one (`gitdir: <path>`, as a linked work tree has).
fn repository(dir: &Path) -> Result<Option<(PathBuf, PathBuf)>, ProdromeError> {
    for tree in dir.ancestors() {
        let dot = tree.join(".git");
        if dot.is_dir() {
            return Ok(Some((tree.to_path_buf(), dot)));
        }
        if dot.is_file() {
            let text = fs::read_to_string(&dot).map_err(|error| ProdromeError::io(&dot, &error))?;
            let named = text.trim_end().strip_prefix("gitdir: ").ok_or_else(|| {
                ProdromeError::Store(format!("{} names no gitdir", dot.display()))
            })?;
            return Ok(Some((tree.to_path_buf(), tree.join(named))));
        }
    }
    Ok(None)
}

/// The width in bytes of an object id in the repository at `git`: 32 where
/// its config says `objectformat = sha256`, and 20 otherwise.
fn id_width(git: &Path) -> usize {
    let sha256 = fs::read_to_string(git.join("config")).is_ok_and(|config| {
        config.lines().any(|line| {
            let line: String = line.chars().filter(|c| !c.is_whitespace()).collect();
            line.eq_ignore_ascii_case("objectformat=sha256")
        })
    });
    if sha256 {
        32
    } else {
        20
    }
}

/// The paths that an index lists, in its order. `width` is the width of an
/// object id.
fn paths(bytes: &[u8], width: usize) -> Result<Vec<String>, &'static str> {
    let word = |at: usize| -> Result<u32, &'static str> {
        bytes
            .get(at..at + 4)
            .and_then(|four| <[u8; 4]>::try_from(four).ok())
            .map(u32::from_be_bytes)
            .ok_or("it ends inside a field")
    };
    if bytes.get(..4) != Some(b"DIRC") {
        return Err("it does not begin with DIRC");
    }
    let version = word(4)?;
    if !(2..=4).contains(&version) {
        return Err("its version is not 2, 3 or 4");
    }
    let count = word(8)?;
    // The fields of an entry before its flags: two times, six words, the
    // object id.
    let fixed = 40 + width;
    let mut at = 12;
    let mut listed: Vec<String> = Vec::new();
    let mut previous: Vec<u8> = Vec::new();
    for _ in 0..count {
        let start = at;
        let flags = bytes
            .get(at + fixed..at + fixed + 2)
            .map(|two| u16::from_be_bytes([two[0], two[1]]))
            .ok_or("it ends inside an entry")?;
        at += fixed + 2;
        if version >= 3 && flags & 0x4000 != 0 {
            at += 2;
        }
        let path = if version == 4 {
            let (strip, read) = varint(bytes.get(at..).ok_or("it ends inside an entry")?)?;
            at += read;
            let suffix = until_nul(bytes, at)?;
            at += suffix.len() + 1;
            let keep = previous
                .len()
                .checked_sub(strip)
                .ok_or("a path strips more than the path before it")?;
            let mut path = previous[..keep].to_vec();
            path.extend_from_slice(suffix);
            path
        } else {
            let name = until_nul(bytes, at)?.to_vec();
            // An entry is padded with one to eight NULs to a multiple of
            // eight bytes.
            at = start + (at - start + name.len() + 8) / 8 * 8;
            name
        };
        if at > bytes.len() {
            return Err("it ends inside an entry");
        }
        listed.push(String::from_utf8_lossy(&path).into_owned());
        previous = path;
    }
    Ok(listed)
}

/// The bytes of `bytes` from `at` to the next NUL.
fn until_nul(bytes: &[u8], at: usize) -> Result<&[u8], &'static str> {
    let rest = bytes.get(at..).ok_or("it ends inside a path")?;
    let end = rest
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("a path has no NUL")?;
    Ok(&rest[..end])
}

/// Git's variable-length number (the "offset encoding" of version 4): its
/// value, and the count of bytes it took.
fn varint(bytes: &[u8]) -> Result<(usize, usize), &'static str> {
    let mut value: usize = 0;
    for (read, byte) in bytes.iter().enumerate() {
        value = value
            .checked_add(usize::from(byte & 0x7f))
            .ok_or("a number overflows")?;
        if byte & 0x80 == 0 {
            return Ok((value, read + 1));
        }
        value = value
            .checked_add(1)
            .and_then(|value| value.checked_mul(128))
            .ok_or("a number overflows")?;
    }
    Err("it ends inside a number")
}

#[cfg(test)]
mod tests {
    use super::{paths, varint};
    use proptest::prelude::*;

    /// Git's encoding of `value` for `varint`.
    fn encode(mut value: usize) -> Vec<u8> {
        let mut out = vec![u8::try_from(value & 0x7f).expect("seven bits")];
        while value >= 0x80 {
            value = (value >> 7) - 1;
            out.push(0x80 | u8::try_from(value & 0x7f).expect("seven bits"));
        }
        out.reverse();
        out
    }

    /// An index of `version` that lists `listed`, in order, with object ids
    /// `width` bytes wide. In version 3, every second entry has the
    /// extended flag and its two more bytes.
    fn index(version: u32, width: usize, listed: &[String]) -> Vec<u8> {
        let mut out = b"DIRC".to_vec();
        out.extend(version.to_be_bytes());
        out.extend(u32::try_from(listed.len()).expect("a few").to_be_bytes());
        let mut previous: &[u8] = b"";
        for (n, path) in listed.iter().enumerate() {
            let start = out.len();
            out.extend(std::iter::repeat_n(7u8, 40 + width));
            let length = u16::try_from(path.len().min(0xfff)).expect("twelve bits");
            let extended = version == 3 && n % 2 == 1;
            let flags = length | if extended { 0x4000 } else { 0 };
            out.extend(flags.to_be_bytes());
            if extended {
                out.extend([0, 0]);
            }
            if version == 4 {
                let shared = previous
                    .iter()
                    .zip(path.as_bytes())
                    .take_while(|(a, b)| a == b)
                    .count();
                out.extend(encode(previous.len() - shared));
                out.extend(&path.as_bytes()[shared..]);
                out.push(0);
            } else {
                out.extend(path.as_bytes());
                let padded = (out.len() - start + 8) / 8 * 8;
                out.resize(start + padded, 0);
            }
            previous = path.as_bytes();
        }
        out.extend(std::iter::repeat_n(0u8, width));
        out
    }

    proptest! {
        #![proptest_config(crate::cases::cases(64))]

        /// Law 81: the paths that an index of version 2, 3 or 4 lists, with
        /// object ids of SHA-1 or SHA-256, are read back exactly, in order.
        #[test]
        fn an_index_lists_exactly_its_paths(
            listed in prop::collection::vec("[a-z.]{1,6}(/[a-z.]{1,6}){0,3}", 0..12),
            version in 2u32..5,
            sha256 in any::<bool>(),
        ) {
            let width = if sha256 { 32 } else { 20 };
            let mut listed = listed;
            listed.sort();
            listed.dedup();
            prop_assert_eq!(paths(&index(version, width, &listed), width), Ok(listed));
        }

        /// Git's variable-length number reads back what it encodes.
        #[test]
        fn a_number_reads_back(value in 0usize..(1 << 40)) {
            let bytes = encode(value);
            prop_assert_eq!(varint(&bytes), Ok((value, bytes.len())));
        }
    }

    /// Law 81 at a store: `verify` names each of `.heads` and `.writes`
    /// that the index of the repository holding the store lists, and no
    /// other file, also from a linked work tree whose `.git` is a file.
    #[test]
    fn verify_names_a_local_file_that_git_tracks() {
        use crate::dag::Finding;
        use crate::event::TodoEvent;
        use crate::policy::Untrusted;
        use crate::reference::Todo;
        use crate::store::EventStore;
        let scratch = std::env::temp_dir().join(format!(
            "prodrome-tracked-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        let repo = scratch.join("repo");
        let store = EventStore::<TodoEvent<Todo>>::new(repo.join("data/store"), Untrusted::none());
        store.init("tracked").expect("begins");
        assert!(
            store.verify().is_empty(),
            "in no repository, nothing is tracked"
        );
        let git = repo.join(".git");
        std::fs::create_dir_all(&git).expect("makes .git");
        let listed = |paths: &[&str]| {
            let paths: Vec<String> = paths.iter().map(|path| (*path).to_owned()).collect();
            index(2, 20, &paths)
        };
        std::fs::write(git.join("index"), listed(&["data/store/.gitignore"])).expect("writes");
        assert!(
            store.verify().is_empty(),
            "the store's own .gitignore is the host's"
        );
        std::fs::write(
            git.join("index"),
            listed(&["data/store/.heads", "data/store/.writes", "data/store/x"]),
        )
        .expect("writes");
        assert_eq!(
            store.verify(),
            vec![Finding::Tracked(".heads"), Finding::Tracked(".writes")]
        );
        // A linked work tree: `.git` is a file that names the git directory.
        let linked = scratch.join("linked");
        std::fs::create_dir_all(&linked).expect("makes the work tree");
        std::fs::write(linked.join(".git"), format!("gitdir: {}\n", git.display()))
            .expect("writes .git");
        let other =
            EventStore::<TodoEvent<Todo>>::new(linked.join("data/store"), Untrusted::none());
        other.init("tracked").expect("begins");
        assert_eq!(
            other.verify(),
            vec![Finding::Tracked(".heads"), Finding::Tracked(".writes")]
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Law 81: a file that is not an index, or that ends inside an entry,
    /// does not parse.
    #[test]
    fn a_cut_or_foreign_index_does_not_parse() {
        let whole = index(2, 20, &[".heads".to_owned(), "objects/a.py".to_owned()]);
        assert!(paths(&whole, 20).is_ok());
        for cut in 0..whole.len() - 20 {
            assert!(paths(&whole[..cut], 20).is_err(), "cut at {cut}");
        }
        assert!(paths(b"PACK\0\0\0\x02\0\0\0\0", 20).is_err());
    }
}
