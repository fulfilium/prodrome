//! The figures of section 9 of `docs/design-table-store.md`: a read, an
//! append and a post on a handle that has read a store of 50,000 objects,
//! with the store opened as shared or as its program's alone. Run BY HAND,
//! in release; nothing under `cargo test` runs it.
//!
//! First lay out the store, outside the tree:
//!
//! ```console
//! $ cargo run --release --example store_doors -p prodrome-core -- gen /tmp/store
//! ```
//!
//! Then run one operation in a new process for each figure, seven times,
//! and take the median of the printed milliseconds:
//!
//! ```console
//! $ for run in 1 2 3 4 5 6 7; do
//!     target/release/examples/store_doors append /tmp/store alone
//!   done
//! ```
//!
//! The operations are `read <dir>`, `append <dir>` and `post <dir> <n>`. The word `alone`
//! opens the store as its program's alone (`Doors::Alone`), and without it
//! the store is opened as shared. The word `cold` drops the page cache
//! (`echo 3 > /proc/sys/vm/drop_caches`, which needs root) after the handle
//! has read the store and before the timed operation, so a cold figure here
//! is a warm handle over a cold disk. Each process first runs one append,
//! so that the store holds a mark of the last hold.
//!
//! The store is one `Genesis` and 49,999 `Change` objects of the reference
//! todo schema, each resting on its entity's last change. One entity has
//! 5,000 writes, and the others are spread over 500 entities. The mean
//! print is 472 bytes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use prodrome::change::mk_change;
use prodrome::event::{canonical_envelope, seal_hash, Envelope, Hash, TodoEvent};
use prodrome::genesis::mk_genesis;
use prodrome::literal::Datetime;
use prodrome::policy::Untrusted;
use prodrome::reference::{mk_authored, Todo};
use prodrome::store::{Doors, EventStore, Replica, Sender};

type Event = TodoEvent<Todo>;
type Store = EventStore<Event>;

const BODY: &str = "A generated record for the measurement of a store of files, short.";

fn event(todo: &str, at: Datetime) -> Event {
    mk_authored(
        todo,
        at,
        "ada",
        "todo",
        at,
        BODY,
        None,
        vec![],
        "",
        "",
        "",
        None,
        vec![],
        vec![],
        "",
    )
    .expect("a valid event")
}

/// The instant `second` seconds into a day in January 2026.
fn instant(second: u32) -> Datetime {
    Datetime::new(
        2026,
        1,
        1,
        second / 3600,
        second % 3600 / 60,
        second % 60,
        0,
    )
    .expect("a real instant")
}

/// A todo name and a day and a microsecond that no earlier run drew.
fn unique() -> (String, u32, u32) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after 1970");
    let day = u32::try_from(now.as_secs() % 28).expect("a day") + 1;
    let micros = now.subsec_micros();
    (
        format!("run-{}-{micros}-{}", now.as_secs(), std::process::id()),
        day,
        micros,
    )
}

/// `n` changes on one new entity, a chain on `genesis`, parents first.
fn chain(genesis: &Hash, n: u32) -> BTreeMap<Hash, Vec<u8>> {
    let (todo, day, micros) = unique();
    let mut prints = BTreeMap::new();
    let mut previous: Option<Hash> = None;
    for at in 0..n {
        let at = Datetime::new(2030, 1, day, at / 3600 % 24, at / 60 % 60, at % 60, micros)
            .expect("a real instant");
        let change = mk_change(
            genesis.clone(),
            previous.iter().cloned().collect(),
            event(&todo, at),
        )
        .expect("a change");
        let object: Envelope<Event> = Envelope::Change(change);
        let name = seal_hash(&object);
        prints.insert(name.clone(), canonical_envelope(&object).into_bytes());
        previous = Some(name);
    }
    prints
}

fn drop_caches() {
    let _ = std::process::Command::new("sync").status();
    fs::write("/proc/sys/vm/drop_caches", "3").expect("drops the page cache (needs root)");
}

/// The store at `dir`, opened as the arguments say.
fn open(dir: &Path, args: &[String]) -> Store {
    let doors = if args.iter().any(|arg| arg == "alone") {
        Doors::Alone
    } else {
        Doors::Shared
    };
    Store::new(dir, Untrusted::none()).with_doors(doors)
}

/// The store laid out at `dir`, as 50,000 appends would have left it.
fn lay_out(dir: &Path) {
    let objects = dir.join("objects");
    fs::create_dir_all(&objects).expect("makes objects/");
    let genesis: Envelope<Event> =
        Envelope::Genesis(mk_genesis("doors", &"0".repeat(32)).expect("a genesis"));
    let root = seal_hash(&genesis);
    let write = |name: &Hash, print: &str| {
        fs::write(objects.join(format!("{}.py", name.as_str())), print).expect("writes");
    };
    write(&root, &canonical_envelope(&genesis));
    let mut last: BTreeMap<String, Hash> = BTreeMap::new();
    let mut big = 0;
    for n in 0..49_999u32 {
        let todo = if n % 10 == 0 && big < 5_000 {
            big += 1;
            "big".to_owned()
        } else {
            format!("e{}", n % 500)
        };
        let deps: Vec<Hash> = last.get(&todo).cloned().into_iter().collect();
        let object: Envelope<Event> = Envelope::Change(
            mk_change(root.clone(), deps, event(&todo, instant(n))).expect("a change"),
        );
        let name = seal_hash(&object);
        write(&name, &canonical_envelope(&object));
        last.insert(todo, name);
    }
    let heads = Store::new(dir, Untrusted::none())
        .record_heads()
        .expect("records");
    // The layout wrote every file with no sync; a later sync of the file
    // system would wait for all of it, so it is synced here, once.
    let _ = std::process::Command::new("sync").status();
    println!("a store of {} objects at {}", heads.count(), dir.display());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(command), Some(dir)) = (args.get(1), args.get(2).map(Path::new)) else {
        eprintln!(
            "usage: store_doors gen <dir> | read <dir> | append <dir> | post <dir> <n>, then [alone] [cold]"
        );
        return;
    };
    if command == "gen" {
        lay_out(dir);
        return;
    }
    let store = open(dir, &args);
    let (todo, day, micros) = unique();
    let primer = event(
        &format!("{todo}-primer"),
        Datetime::new(2031, 1, day, 0, 0, 0, micros).expect("an instant"),
    );
    store
        .append(primer)
        .expect("appends, so that a mark of the last hold exists");
    let genesis = store
        .dag()
        .expect("reads")
        .geneses()
        .into_iter()
        .next()
        .expect("a genesis");
    let cold = args.iter().any(|arg| arg == "cold");
    let elapsed = match command.as_str() {
        "read" => {
            if cold {
                drop_caches();
            }
            let start = Instant::now();
            store.held().expect("reads");
            start.elapsed()
        }
        "append" => {
            let event = event(
                &todo,
                Datetime::new(2031, 1, day, 1, 0, 0, micros).expect("an instant"),
            );
            if cold {
                drop_caches();
            }
            let start = Instant::now();
            store.append(event).expect("appends");
            start.elapsed()
        }
        "post" => {
            let n: u32 = args
                .get(3)
                .and_then(|n| n.parse().ok())
                .expect("post takes a count");
            let prints = chain(&genesis, n);
            let seeds: BTreeSet<Hash> = prints.keys().cloned().collect();
            if cold {
                drop_caches();
            }
            let start = Instant::now();
            store
                .receive(seeds, &|name| prints.get(name).cloned(), Sender::Stranger)
                .expect("receives");
            start.elapsed()
        }
        other => {
            eprintln!("unknown operation {other}");
            return;
        }
    };
    println!("{:.3}", elapsed.as_secs_f64() * 1000.0);
}
