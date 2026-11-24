//! Where one `syncfs` stands for a sync of each file: the kernel and the
//! file system that a batch of placements is staged on.
//!
//! A batch can be made durable by one `syncfs` ([`super::Staged`]) only
//! where its answer of success means that every staged byte is durable, and
//! where it reports an error of writeback of a file that was closed before
//! the sync. Linux reports such an error from 5.8 on: the kernel keeps one
//! error counter for each file system, and samples it when a file is opened.
//! On ext4, XFS, btrfs and ZFS, the file system writes back what `syncfs`
//! asks for. On a FUSE mount, the kernel does not always pass the sync to
//! the daemon, and on overlayfs an error of writeback lands on the upper
//! file system, whose counter the overlay's sync does not read: there, and
//! on every other file system, the store syncs each file.

use std::fs::File;

/// The first kernel whose `syncfs` reports an error of writeback.
const FIRST: (u32, u32) = (5, 8);

/// The magic numbers (`statfs.f_type`) of the file systems where one
/// `syncfs` stands for a sync of each file: ext2, ext3 and ext4 (which
/// share one), XFS, btrfs and ZFS.
const KNOWN: [i128; 4] = [0xEF53, 0x5846_5342, 0x9123_683E, 0x2FC1_2FC1];

/// Does one `syncfs` through `handle` stand for a sync of each file of its
/// file system? Yes where the running kernel reports a lost write and the
/// file system is one of [`KNOWN`]. The kernel's release is read by
/// `uname`, which opens no file.
pub(super) fn stands_for_each_file(handle: &File) -> bool {
    let release = rustix::system::uname();
    let release = release.release().to_string_lossy();
    admits(&release)
        && rustix::fs::fstatfs(handle).is_ok_and(|stat| KNOWN.contains(&i128::from(stat.f_type)))
}

/// Does a kernel of the release `release` (such as `6.18.44-generic`)
/// report a lost write from `syncfs`?
fn admits(release: &str) -> bool {
    let mut parts = release
        .split(|c: char| !c.is_ascii_digit())
        .map(str::parse::<u32>);
    matches!(
        (parts.next(), parts.next()),
        (Some(Ok(major)), Some(Ok(minor))) if (major, minor) >= FIRST
    )
}

#[cfg(test)]
mod tests {
    use super::admits;

    /// Law 83: one sync is taken under Linux 5.8 and later, and a sync of
    /// each file under an earlier kernel or a release that does not parse.
    #[test]
    fn one_sync_only_where_syncfs_reports_a_lost_write() {
        for release in [
            "5.8.0",
            "5.10.209-generic",
            "6.1",
            "6.18.44-fc-v726",
            "10.0.1",
        ] {
            assert!(admits(release), "{release}");
        }
        for release in [
            "5.7.19",
            "4.19.0-25-amd64",
            "3.10.0-1160.el7.x86_64",
            "",
            "linux",
            "6",
        ] {
            assert!(!admits(release), "{release:?}");
        }
    }
}
