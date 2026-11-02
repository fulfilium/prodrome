//! A map over a vector that runs on every core the host reports, and
//! answers in the vector's order.

use std::num::NonZeroUsize;
use std::thread;

/// The fewest items a thread is started for. Below this, the work on the
/// items costs less than a thread does.
const SHARE: usize = 64;

/// `items.into_iter().map(work).collect()`, with the items shared among the
/// cores that [`thread::available_parallelism`] reports: item `i` goes to
/// thread `i % threads`, so a run of large items is shared too, and the
/// answers come back in the items' order. The calling thread does all the
/// work where the host reports one core or none (a browser does), and where
/// there are fewer than two shares of items. A panic in `work` is resumed
/// on the calling thread, as it would be without threads.
pub(crate) fn spread<T: Send, U: Send>(items: Vec<T>, work: impl Fn(T) -> U + Sync) -> Vec<U> {
    let threads = thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(items.len() / SHARE);
    if threads < 2 {
        return items.into_iter().map(work).collect();
    }
    let len = items.len();
    let mut shares: Vec<Vec<T>> = (0..threads)
        .map(|_| Vec::with_capacity(len.div_ceil(threads)))
        .collect();
    for (at, item) in items.into_iter().enumerate() {
        shares[at % threads].push(item);
    }
    let work = &work;
    let mut done: Vec<std::vec::IntoIter<U>> = thread::scope(|scope| {
        let running: Vec<_> = shares
            .into_iter()
            .map(|share| scope.spawn(move || share.into_iter().map(work).collect::<Vec<U>>()))
            .collect();
        running
            .into_iter()
            .map(|thread| {
                thread
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                    .into_iter()
            })
            .collect()
    });
    (0..len)
        .filter_map(|at| done[at % threads].next())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::spread;

    /// The map in parallel is the map: the same answers in the same order,
    /// below one share, at the edge of two, and over many.
    #[test]
    fn a_spread_is_the_map() {
        for len in [0, 1, 63, 64, 127, 128, 129, 1_000, 4_097] {
            let items: Vec<u64> = (0..len).collect();
            let work = |item: u64| item * item + 7;
            assert_eq!(
                spread(items.clone(), work),
                items.into_iter().map(work).collect::<Vec<_>>(),
                "{len} items"
            );
        }
    }
}
