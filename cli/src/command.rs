//! The verbs, as clap declares them.
//!
//! Nothing here decides anything: the types are the parse boundary and the
//! whole of the command line's grammar, and [`crate::run`] is what reads them.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// `prodrome` — a store of todos, priced as functions of time.
#[derive(Debug, Parser)]
#[command(name = "prodrome", version, about, long_about = None)]
pub struct Cli {
    /// The store to read or write. Defaults to `./roadmap` when that directory
    /// exists, and to the current directory otherwise.
    #[arg(long, global = true, value_name = "DIR")]
    pub store: Option<PathBuf>,

    /// Actors whose lifecycle and repricing events only CLAIM (§5). This is
    /// the READER's policy, not the store's: a store holds no policy, so
    /// naming a roster here is how one reader asks for the confirmed reading
    /// and the claimed one side by side.
    #[arg(
        long,
        global = true,
        value_name = "NAME,…",
        value_delimiter = ',',
        env = "PRODROME_UNTRUSTED"
    )]
    pub untrusted: Vec<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a store: `<dir>/objects/`, holding the prodrome's genesis. The
    /// objects are the whole store; its tips are derived from them.
    Init {
        #[arg(value_name = "DIR")]
        dir: PathBuf,
    },

    /// Append a todo: a `Created`, the `SpecRevised` that prices it, and a
    /// content record when there is a `--detail` to record.
    Add {
        #[arg(value_name = "ID")]
        id: String,
        /// What the todo asks for. Stored as the `Created` event's text and as
        /// the record's body.
        #[arg(long, value_name = "TEXT")]
        body: String,
        /// Longer content, stored on a record. Without it no record is
        /// written: an empty record is not content.
        #[arg(long, default_value = "", value_name = "TEXT")]
        detail: String,
        #[command(flatten)]
        price: Price,
        #[command(flatten)]
        stamp: Stamp,
    },

    /// Append a `Completed`.
    Done {
        #[arg(value_name = "ID")]
        id: String,
        #[command(flatten)]
        stamp: Stamp,
    },

    /// Append a `Cancelled`.
    Cancel {
        #[arg(value_name = "ID")]
        id: String,
        #[command(flatten)]
        stamp: Stamp,
    },

    /// Append a `Reopened`.
    Reopen {
        #[arg(value_name = "ID")]
        id: String,
        #[command(flatten)]
        stamp: Stamp,
    },

    /// Append a `SpecRevised`: a new price, in force from its instant on.
    Revise {
        #[arg(value_name = "ID")]
        id: String,
        #[command(flatten)]
        price: Price,
        #[command(flatten)]
        stamp: Stamp,
    },

    /// The open todos at an instant, most urgent first.
    List {
        /// The instant to fold at, ISO (`2026-09-09` or
        /// `2026-09-09T17:00:00`). Defaults to the machine's clock.
        #[arg(long, value_name = "ISO")]
        at: Option<String>,
    },

    /// One todo, as the folds see it at an instant.
    Show {
        #[arg(value_name = "ID")]
        id: String,
        /// The instant to fold at, ISO. Defaults to the machine's clock.
        #[arg(long, value_name = "ISO")]
        at: Option<String>,
    },

    /// Check the store, changing nothing (§3): every object rehashes, every
    /// parent exists, no cycle, nothing in `objects/` that is not an object.
    Verify,

    /// `verify`, after moving every object file that fails its hash, and is
    /// no husk at the store's schema, out of `objects/` into `quarantine/`,
    /// so the store writes again (§3): its
    /// reads answer throughout, leaving such a file out, but every write
    /// refuses while one is there. The report names each one until it is
    /// restored from a replica.
    Fsck,

    /// Attest the store: a `Snapshot` of its tips, chained to the last one,
    /// whose name commits to everything written so far (§3).
    Snapshot,

    /// Join a legacy store's heads into one `Woven` carrying no event. A
    /// store of changes has nothing to weave.
    Weave,
}

/// The three fields every write shares beyond the verb's own.
#[derive(Debug, Args)]
pub struct Stamp {
    /// Who is writing. Defaults to `$PRODROME_ACTOR`, then `$USER`.
    #[arg(long, value_name = "NAME", env = "PRODROME_ACTOR")]
    pub actor: Option<String>,
    /// The instant to STAMP ON the event, ISO. Defaults to the machine's
    /// clock. An event's `at` is data — the store has no clock — so a writer
    /// recording something that happened earlier says so here.
    #[arg(long, value_name = "ISO")]
    pub at: Option<String>,
    /// The event's note.
    #[arg(long, default_value = "", value_name = "TEXT")]
    pub note: String,
}

/// What a todo is worth as a function of time, in the two shapes a command
/// line can state: a constant, or a decay onto a date.
///
/// A GROUP AND NOT TWO OPTIONS: `--priority` and `--deadline` are two ways to
/// say one thing, and clap refuses both at once rather than this code choosing
/// a winner.
#[derive(Debug, Args)]
#[group(id = "price", required = false, multiple = false)]
pub struct Price {
    /// A constant fulfillment, 0–100, where LOW IS URGENT: 10 is
    /// life-threatening, 35–45 a real consequence within weeks, 70–85 a
    /// nice-to-have, 90+ parked.
    #[arg(long, group = "price", value_name = "N")]
    pub priority: Option<u8>,

    /// A date to decay onto, `YYYY-MM-DD`. The instant is 17:00 on that day.
    #[arg(long, group = "price", value_name = "YYYY-MM-DD")]
    pub deadline: Option<String>,

    /// Fulfillment before the lead-up begins, 0–98. Only with `--deadline`.
    #[arg(long, default_value_t = Price::DEFAULT_START, value_name = "N")]
    pub start: u8,

    /// Fulfillment at the deadline and after it, 0–100. Only with
    /// `--deadline`.
    #[arg(long, default_value_t = Price::DEFAULT_END, value_name = "N")]
    pub end: u8,

    /// How many days before the deadline the decay begins. Only with
    /// `--deadline`.
    #[arg(long, default_value_t = Price::DEFAULT_LEAD_UP_DAYS, value_name = "DAYS")]
    pub lead_up_days: f64,
}

impl Price {
    /// The defaults a deadline takes, named once so that `/price … --deadline`
    /// on an issue and `--deadline` here write the same decay.
    pub const DEFAULT_START: u8 = 55;
    pub const DEFAULT_END: u8 = 5;
    pub const DEFAULT_LEAD_UP_DAYS: f64 = 3.0;
}
