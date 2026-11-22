//! `prodrome` — a command line over a Prodrome store.
//!
//! THE HOST IS HERE AND NOWHERE ELSE. The core is generic over what a record
//! holds (§4) and over whose word binds (§5); this crate answers both, with
//! the reference payload ([`prodrome::reference::Todo`]) and the reference
//! policy ([`prodrome::policy::Untrusted`]). It also owns the one thing the
//! core refuses to have: a clock. An event's `at` is data, and a fold is a
//! query at an instant the caller names — so `--at` is how a caller names one
//! and the machine's clock is only the default.
//!
//! Every verb answers with an [`Outcome`]: the text to print and whether the
//! store was found healthy. Nothing here writes to a stream, so a test drives
//! a verb and reads what it said.

#![forbid(unsafe_code)]

pub mod command;
pub mod price;
pub mod render;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use prodrome::dag::Finding;
use prodrome::event::{
    mk_cancelled, mk_completed, mk_created, mk_reopened, mk_spec_revised, Actor, TodoEvent, TodoId,
};
use prodrome::fpl::FplError;
use prodrome::literal::{Datetime, ProdromeError};
use prodrome::policy::Untrusted;
use prodrome::reference::{mk_authored, Todo};
use prodrome::registers;
use prodrome::store::EventStore;
use prodrome::view::{entries, Entry};

use command::{Cli, Command, Price, Stamp};

/// The store this binary reads and writes: the reference payload, under the
/// reference policy.
pub type Store = EventStore<TodoEvent<Todo>, Untrusted>;

/// A refusal, as a value. The three the core has, plus the one a command line
/// adds: an argument that means nothing.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Store(#[from] ProdromeError),
    #[error("{0}")]
    Fpl(#[from] FplError),
    #[error("{0}")]
    Usage(String),
}

impl Error {
    /// A refusal of the input, in the words it gives.
    pub fn usage(message: impl Into<String>) -> Error {
        Error::Usage(message.into())
    }
}

/// What a verb answers: what to print, whether it found what it looked at
/// to be in order, and what its reading left out. `ok` is false only where a
/// verb makes a JUDGEMENT that failed — `verify` with findings — and never
/// for a refusal, which is an [`Error`].
///
/// A reading of the store is total (SPEC law 45): a file that is no object
/// is left out with everything resting on it, so a todo whose completion
/// rests on it reads as open. It is never left out silently: `left_out`
/// says what, one line each, for the standard error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub text: String,
    pub ok: bool,
    pub left_out: Vec<String>,
}

impl Outcome {
    /// An answer: `text`, and a verdict that passed.
    pub fn said(text: impl Into<String>) -> Outcome {
        Outcome {
            text: text.into(),
            ok: true,
            left_out: Vec::new(),
        }
    }

    /// This answer, read from `store`, with what its reading left out.
    fn reading(self, store: &Store) -> Result<Outcome, Error> {
        Ok(Outcome {
            left_out: left_out(store)?,
            ..self
        })
    }
}

/// What a reading of `store` leaves out ([`prodrome::dag::Dag::excluded`]),
/// one line each: the name, why, and how many objects rest on it.
fn left_out(store: &Store) -> Result<Vec<String>, Error> {
    Ok(store
        .dag()?
        .excluded()
        .into_iter()
        .map(|out| {
            let what = match out.why {
                Some(why) => Finding::Unread {
                    name: out.name,
                    why,
                }
                .to_string(),
                None => format!("object {} is not in the store", out.name),
            };
            let resting = out.resting.len();
            format!(
                "left out {what}, and the {resting} object{} resting on it (`prodrome verify`)",
                if resting == 1 { "" } else { "s" }
            )
        })
        .collect())
}

/// Where the store is.
///
/// `./roadmap` before `.`, because a repository that keeps its todos in a
/// subdirectory is the case this binary was written for and typing `--store`
/// at every invocation would be the tax for it. Nothing is created here: a
/// path that is not a store is a store with no objects, and `verify` says so.
pub fn store_root(named: Option<&Path>) -> PathBuf {
    match named {
        Some(path) => path.to_path_buf(),
        None => {
            let roadmap = PathBuf::from("roadmap");
            if roadmap.is_dir() {
                roadmap
            } else {
                PathBuf::from(".")
            }
        }
    }
}

/// The reader's policy: the roster named on the command line or in
/// `PRODROME_UNTRUSTED`, and `Untrusted::none()` — standing behind every
/// writer — when neither named one.
fn policy_of(untrusted: &[String]) -> Result<Untrusted, Error> {
    let mut actors = Vec::with_capacity(untrusted.len());
    for name in untrusted {
        actors.push(Actor::new(name.clone())?);
    }
    Ok(Untrusted::of(actors))
}

/// Who is writing: `--actor`, then `$PRODROME_ACTOR` (clap reads it into the
/// same field), then `$USER`. There is no fourth fallback: an event without an
/// actor is not an event this format can hold, so a machine with no `$USER`
/// gets a refusal rather than a name this program invented.
fn actor_of(stamp: &Stamp) -> Result<String, Error> {
    stamp
        .actor
        .clone()
        .or_else(|| std::env::var("USER").ok().filter(|name| !name.is_empty()))
        .ok_or_else(|| Error::usage("no actor: pass --actor, or set PRODROME_ACTOR or USER"))
}

/// One store, folded at one instant, with the one thing a §6.7 row does not
/// carry and a reader wants: what each todo says.
pub struct Reading {
    pub at: Datetime,
    pub untrusted: Vec<String>,
    pub entries: Vec<Entry<TodoEvent<Todo>>>,
    bodies: BTreeMap<TodoId, String>,
    details: BTreeMap<TodoId, String>,
}

impl Reading {
    /// What the todo asks for: the winning record's body, and the `Created`
    /// event's text for a todo with no record.
    pub fn body(&self, todo: &TodoId) -> &str {
        self.bodies.get(todo).map_or("", String::as_str)
    }

    pub fn detail(&self, todo: &TodoId) -> &str {
        self.details.get(todo).map_or("", String::as_str)
    }
}

/// Fold the store at `at` under its own policy, and gather what the renderers
/// read.
///
/// The roster comes off the STORE's policy rather than off the command line
/// again: one reading is taken under one policy, and a summary line that
/// quoted its own copy of the argument could say something the fold did not
/// do.
pub fn read(store: &Store, at: Datetime) -> Result<Reading, Error> {
    let nodes = store.dag()?.nodes()?;
    let events: Vec<TodoEvent<Todo>> = nodes
        .iter()
        .filter_map(|node| node.event().cloned())
        .collect();

    let mut bodies = BTreeMap::new();
    for event in &events {
        if let TodoEvent::Created(e) = event {
            bodies.insert(e.todo.clone(), e.text.clone());
        }
    }
    let mut details = BTreeMap::new();
    let state = registers::fold(&nodes);
    for prodrome in state.prodromes().values() {
        for (todo, records) in prodrome::fold::content(prodrome, prodrome::fpl::instant_of(at)) {
            let joined = |field: fn(&Todo) -> &str| {
                let texts: Vec<&str> = records.iter().map(|r| field(&r.payload)).collect();
                texts.join(" | ")
            };
            let body = joined(|todo| todo.body.as_str());
            if !body.is_empty() {
                bodies.insert(todo.clone(), body);
            }
            details.insert(todo, joined(|todo| todo.detail.as_str()));
        }
    }

    Ok(Reading {
        at,
        untrusted: store
            .policy()
            .actors()
            .iter()
            .map(|actor| actor.as_str().to_owned())
            .collect(),
        entries: entries(&nodes, at, store.policy())?,
        bodies,
        details,
    })
}

/// The todos the chain has ever named — what an id is checked against before a
/// write, so a typo appends nothing.
fn known(store: &Store) -> Result<Vec<TodoId>, Error> {
    Ok(store
        .events()?
        .iter()
        .filter_map(|event| match event {
            TodoEvent::Created(e) => Some(e.todo.clone()),
            _ => None,
        })
        .collect())
}

fn require_known(store: &Store, id: &TodoId) -> Result<(), Error> {
    if known(store)?.contains(id) {
        Ok(())
    } else {
        Err(Error::usage(format!(
            "no todo {:?} in this store",
            id.as_str()
        )))
    }
}

/// The shape `mk_completed`, `mk_cancelled` and `mk_reopened` share — §4's
/// three lifecycle kinds are one record type in the core for the same reason.
type MkLifecycle = fn(&str, Datetime, &str, &str) -> Result<TodoEvent<Todo>, ProdromeError>;

/// A lifecycle write — `done`, `cancel`, `reopen` — which differ only in the
/// constructor.
fn lifecycle(store: &Store, id: &str, stamp: &Stamp, make: MkLifecycle) -> Result<Outcome, Error> {
    let todo = TodoId::new(id)?;
    require_known(store, &todo)?;
    let at = price::instant(stamp.at.as_deref())?;
    let actor = actor_of(stamp)?;
    let digest = store.append(make(id, at, &actor, &stamp.note)?)?;
    Ok(Outcome::said(digest.as_str()))
}

/// Run one verb.
///
/// # Errors
///
/// Every refusal is an [`Error`]: an argument that means nothing, a todo the
/// store does not hold, or the store itself refusing.
pub fn run(cli: &Cli) -> Result<Outcome, Error> {
    // Opening a store touches no disk — it is a root and a policy — so `init`
    // sits in the same match as everything else even though its store is the
    // one that does not exist yet.
    let root = store_root(cli.store.as_deref());
    let store = Store::new(&root, policy_of(&cli.untrusted)?);

    match &cli.command {
        Command::Init { dir } => init(dir, store.policy().clone()),

        Command::Add {
            id,
            body,
            detail,
            price: spec,
            stamp,
        } => add(&store, id, body, detail, spec, stamp),

        Command::Done { id, stamp } => lifecycle(&store, id, stamp, mk_completed),
        Command::Cancel { id, stamp } => lifecycle(&store, id, stamp, mk_cancelled),
        Command::Reopen { id, stamp } => lifecycle(&store, id, stamp, mk_reopened),

        Command::Revise { id, price, stamp } => {
            let todo = TodoId::new(id)?;
            require_known(&store, &todo)?;
            let spec = price::term_of(price)?
                .ok_or_else(|| Error::usage("revise needs a new spec: --priority or --deadline"))?;
            let at = price::instant(stamp.at.as_deref())?;
            let actor = actor_of(stamp)?;
            let digest = store.append(mk_spec_revised(id, at, &actor, spec, &stamp.note)?)?;
            Ok(Outcome::said(digest.as_str()))
        }

        Command::List { at } => {
            let reading = read(&store, price::instant(at.as_deref())?)?;
            Outcome::said(render::list(&reading)).reading(&store)
        }

        Command::Show { id, at } => {
            let todo = TodoId::new(id)?;
            let reading = read(&store, price::instant(at.as_deref())?)?;
            render::show(&reading, &todo)
                .map(Outcome::said)
                .ok_or_else(|| Error::usage(format!("no todo {id:?} in this store")))?
                .reading(&store)
        }

        Command::Verify => report(&store, &store.verify()),

        Command::Fsck => report(&store, &store.fsck()),

        Command::Snapshot => Ok(Outcome::said(store.snapshot()?.as_str())),

        Command::Weave => {
            let heads = store.dag()?.tips_in(&None);
            if heads.len() < 2 {
                return Ok(Outcome::said(format!(
                    "{} head: nothing to weave",
                    heads.len()
                )));
            }
            // NO EVENT AND NO ACTOR. A merge asserts structure, not a fact
            // about a todo (§3), so there is nothing here for a `--actor` to
            // be the author of.
            let digest = store.merge(None, None)?;
            Ok(Outcome::said(digest.as_str()))
        }
    }
}

/// `init`: an `objects/` directory holding the prodrome's `Genesis`, labelled
/// with the directory's name. The objects are the whole store: its tips are
/// derived from them (§3), so no file names a head.
/// A check's findings as an outcome: `ok:` with the store's size where
/// there are none, and each finding on its own line, failed, where there are.
fn report(store: &Store, findings: &[Finding]) -> Result<Outcome, Error> {
    if findings.is_empty() {
        let objects = store.dag()?.objects().len();
        let heads = store.tips()?.len();
        return Ok(Outcome::said(format!(
            "ok: {objects} objects, {heads} head{}",
            if heads == 1 { "" } else { "s" }
        )));
    }
    Ok(Outcome {
        text: findings
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        ok: false,
        left_out: Vec::new(),
    })
}

fn init(dir: &Path, policy: Untrusted) -> Result<Outcome, Error> {
    if dir.join("objects").is_dir() {
        return Err(Error::usage(format!(
            "{} is already a store",
            dir.display()
        )));
    }
    std::fs::create_dir_all(dir.join("objects")).map_err(|e| ProdromeError::Io {
        path: dir.display().to_string(),
        message: e.to_string(),
    })?;
    let label = dir
        .file_name()
        .map_or(String::new(), |name| name.to_string_lossy().into_owned());
    Store::new(dir, policy).init(&label)?;
    Ok(Outcome::said(format!("initialised {}", dir.display())))
}

/// `add`: the `Created` that names the todo, the `SpecRevised` that prices it
/// when a price was given, and a record when there is a `--detail` to hold.
///
/// THREE OBJECTS AT MOST, AND THE PRICE LIVES IN ONE PLACE. A record can carry
/// a spec too (§6.2 reads one from either), and writing it in both would be
/// two spellings of one price to keep in step.
fn add(
    store: &Store,
    id: &str,
    body: &str,
    detail: &str,
    price_args: &Price,
    stamp: &Stamp,
) -> Result<Outcome, Error> {
    let todo = TodoId::new(id)?;
    if known(store)?.contains(&todo) {
        return Err(Error::usage(format!(
            "todo {id:?} is already in this store"
        )));
    }
    if body.trim().is_empty() {
        return Err(Error::usage("--body cannot be empty"));
    }
    let at = price::instant(stamp.at.as_deref())?;
    let actor = actor_of(stamp)?;

    let mut written = vec![store.append(mk_created(id, at, &actor, body, &stamp.note)?)?];
    if let Some(spec) = price::term_of(price_args)? {
        written.push(store.append(mk_spec_revised(id, at, &actor, spec, "")?)?);
    }
    if !detail.is_empty() {
        let record = mk_authored(
            id,
            at,
            &actor,
            "todo",
            at,
            body,
            None,
            Vec::new(),
            "",
            "",
            detail,
            None,
            Vec::new(),
            Vec::new(),
            "",
        )?;
        written.push(store.append(record)?);
    }
    Ok(Outcome::said(
        written
            .iter()
            .map(|digest| digest.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    ))
}
