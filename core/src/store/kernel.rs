//! Whether the running Linux kernel lets one `syncfs` stand for a sync of
//! each file: its version, read once for each process.
//!
//! A batch of placements makes its staged bytes durable with one `syncfs`
//! ([`super::Staged`]). Since Linux 5.8, `syncfs` reports every error of
//! writeback on the file system after the handle that it is called on was
//! opened. Before 5.8 it answers success after such an error, so a write
//! that the disk lost would not be reported, and the store refuses to
//! write there.

use std::sync::OnceLock;

use crate::literal::ProdromeError;

/// The first kernel whose `syncfs` reports an error of writeback.
const FIRST: (u32, u32) = (5, 8);

/// Does the running kernel's `syncfs` report a lost write? Read once, from
/// `/proc/sys/kernel/osrelease`.
///
/// # Errors
///
/// A kernel before 5.8, or a version that cannot be read, in words that
/// say why the store refuses.
pub(super) fn reports_lost_writes() -> Result<(), ProdromeError> {
    static ANSWER: OnceLock<Result<(), String>> = OnceLock::new();
    ANSWER
        .get_or_init(|| {
            let release = std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .map_err(|error| format!("the kernel's version cannot be read: {error}"))?;
            admits(release.trim())
        })
        .clone()
        .map_err(|why| {
            ProdromeError::Store(format!(
                "the store does not write here: it makes a batch durable with one syncfs, \
                 which reports a write that the disk lost only on Linux 5.8 or later, and {why}"
            ))
        })
}

/// Does a kernel of the release `release` (such as `6.18.44-generic`)
/// report a lost write from `syncfs`? The refusal says the release.
fn admits(release: &str) -> Result<(), String> {
    let mut parts = release
        .split(|c: char| !c.is_ascii_digit())
        .map(str::parse::<u32>);
    match (parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor))) if (major, minor) >= FIRST => Ok(()),
        (Some(Ok(_)), Some(Ok(_))) => Err(format!("this kernel is {release}")),
        _ => Err(format!("the kernel's version {release:?} does not parse")),
    }
}

#[cfg(test)]
mod tests {
    use super::admits;

    /// Law 83: the store writes on Linux 5.8 and later, and refuses an
    /// earlier kernel, or a release it cannot read, naming the release.
    #[test]
    fn the_store_writes_only_where_syncfs_reports_a_lost_write() {
        for release in [
            "5.8.0",
            "5.10.209-generic",
            "6.1",
            "6.18.44-fc-v726",
            "10.0.1",
        ] {
            assert_eq!(admits(release), Ok(()), "{release}");
        }
        for release in ["5.7.19", "4.19.0-25-amd64", "3.10.0-1160.el7.x86_64"] {
            let refusal = admits(release).expect_err(release);
            assert!(refusal.contains(release), "{refusal}");
        }
        for release in ["", "linux", "6"] {
            assert!(admits(release).is_err(), "{release:?}");
        }
    }
}
