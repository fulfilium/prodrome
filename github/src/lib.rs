//! `prodrome-github`: one GitHub webhook delivery, as the events it means.
//!
//! THE MIRROR IS A FUNCTION OF THE PAYLOAD AND THE STORE, AND OF NOTHING ELSE.
//! Every instant written here is read off the payload (`created_at`,
//! `updated_at`, `closed_at`), never off a clock, and nothing here reaches a
//! network. So the same delivery, applied to the same store, always means the
//! same events — which is what lets a replay append nothing: `append` writes
//! nothing for an event its prodrome already holds (SPEC §3).
//!
//! The payload is the JSON a workflow finds at `$GITHUB_EVENT_PATH`. Three
//! event names are read, `issues`, `issue_comment` and `pull_request`, and told
//! apart by the payload itself (only a comment delivery carries `comment`, and
//! only a pull request delivery carries `pull_request` at its top level), so
//! the input is one file and a test is one fixture.
//!
//! WHO WRITES WHAT. Facts GitHub asserts — an issue was opened, retitled,
//! closed, reopened — are written as [`MIRROR`]. A `/price` comment is written
//! as the commenter, and only when GitHub says the commenter is the
//! repository's owner, a member or a collaborator; anyone else's `/price` is
//! ignored before its text is even parsed. A proposal from the pricing step is
//! written as [`BOT`], which a reader passes to `--untrusted` to see it as the
//! CLAIM it is (§5). The merge of a pull request whose description names
//! items in `Completes:` lines completes those items, as [`MIRROR`]
//! ([`completes`]).

use std::collections::BTreeSet;
use std::path::Path;

use prodrome::dag::Dag;
use prodrome::event::{
    mk_completed, mk_created, mk_reopened, mk_spec_revised, Envelope, TodoEvent, TodoId,
};
use prodrome::fpl::{self, datetime_of, mk_ref};
use prodrome::literal::{Datetime, ProdromeError};
use prodrome::reference::{mk_authored, mk_source, Todo};
use prodrome::registers::{self, Folded, Genesis};
use prodrome::term::Term;
use serde_json::Value;

use prodrome_cli::command::Price as PriceArgs;
use prodrome_cli::{price, store_root, Error, Outcome, Store};

pub mod completes;

/// The actor every mirrored fact is written as.
pub const MIRROR: &str = "github-mirror";

/// The actor a pricing proposal is written as — a CLAIM under any reader who
/// names it untrusted, which is the reading the workflows document.
pub const BOT: &str = "pricing-bot";

/// The price an issue is mirrored with before anyone has priced it: 80%,
/// between "a useful feature someone is waiting on" and "nice to have" on
/// PRICING.md's scale — present in the list, and urgent to nobody.
pub const NEUTRAL: u8 = 80;

/// The record category every mirrored issue carries.
pub const CATEGORY: &str = "issue";

/// The note on a maintainer's repricing that accepted a proposal.
const ACCEPTED: &str = "accepted pricing-bot's proposal";

/// The longest rationale a proposal's note keeps. The note is a label beside a
/// claim, and the full reasoning is on the issue, in the comment it came from.
const NOTE_LIMIT: usize = 280;

// --- the payload, as types ---------------------------------------------------

/// The issue a delivery is about, as far as the mirror reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub created_at: Datetime,
    pub updated_at: Datetime,
    pub closed_at: Option<Datetime>,
}

impl Issue {
    /// The item this issue is mirrored as: `gh-<number>`, the id the viewer's
    /// `#/todo/<id>` URL names.
    pub fn todo(&self) -> String {
        todo_of(self.number)
    }
}

/// `gh-<number>`.
pub fn todo_of(number: u64) -> String {
    format!("gh-{number}")
}

/// What GitHub says a commenter is to the repository. Only the three that
/// carry write access may price; the rest are one case, because the mirror
/// asks one question of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Maintainer,
    Outsider,
}

impl Standing {
    /// `author_association`, read strictly: OWNER, MEMBER and COLLABORATOR
    /// are maintainers, and every other value — including one GitHub adds
    /// later — is an outsider.
    pub fn of(association: &str) -> Standing {
        match association {
            "OWNER" | "MEMBER" | "COLLABORATOR" => Standing::Maintainer,
            _ => Standing::Outsider,
        }
    }
}

/// A comment on an issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub login: String,
    pub standing: Standing,
    pub body: String,
    pub at: Datetime,
}

/// What happened to the issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Opened,
    /// Edited, and whether the TITLE was among what changed — the only part
    /// of an issue the mirror holds.
    Edited {
        title_changed: bool,
    },
    Closed,
    Reopened,
    Commented(Comment),
    /// Labelled, assigned, pinned, …: nothing the mirror records.
    Other(String),
}

/// One webhook delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub issue: Issue,
    pub action: Action,
    /// `issue_comment` fires for pull requests too, and the payload's `issue`
    /// is then the pull request. Those are not mirrored.
    pub pull_request: bool,
}

/// A price as a `/price` comment states it.
#[derive(Debug, Clone, PartialEq)]
pub enum Price {
    /// `/price 40`: a constant fulfillment, in percent.
    Flat(u8),
    /// `/price 30 --deadline 2026-10-15 [--end 5] [--lead-up 3]`: a decay onto
    /// 17:00 that day, the same decay `prodrome revise --deadline` writes.
    Deadline {
        start: u8,
        day: String,
        end: u8,
        lead_up_days: f64,
    },
    /// `/price ref gh-7`: exactly as urgent as another item.
    Ref(TodoId),
    /// `/price accept`: the pricing bot's latest proposal, re-issued as the
    /// commenter's own.
    Accept,
}

// --- reading the payload -----------------------------------------------------

pub(crate) fn field<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |value, key| value.get(key))
}

fn text(value: &Value, path: &[&str]) -> Result<String, Error> {
    field(value, path)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::usage(format!("payload: no string at {}", path.join("."))))
}

/// A GitHub timestamp, `2026-09-27T12:34:56Z`, as the UTC instant it names.
pub fn instant_of(stamp: &str) -> Result<Datetime, Error> {
    let naive = stamp.strip_suffix('Z').ok_or_else(|| {
        Error::usage(format!(
            "payload: expected a UTC timestamp ending in Z, got {stamp:?}"
        ))
    })?;
    Ok(datetime_of(fpl::parse_iso(naive)?)?)
}

fn stamp(value: &Value, path: &[&str]) -> Result<Datetime, Error> {
    instant_of(&text(value, path)?)
}

/// The delivery a payload describes.
///
/// # Errors
///
/// A payload missing a field the mirror reads is a refusal: better nothing
/// written than an item built from a guess.
pub fn delivery_of(payload: &Value) -> Result<Delivery, Error> {
    let number = field(payload, &["issue", "number"])
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::usage("payload: no issue.number"))?;
    let closed_at = match field(payload, &["issue", "closed_at"]) {
        Some(Value::String(at)) => Some(instant_of(at)?),
        _ => None,
    };
    let issue = Issue {
        number,
        title: text(payload, &["issue", "title"])?,
        url: text(payload, &["issue", "html_url"])?,
        author: text(payload, &["issue", "user", "login"])?,
        created_at: stamp(payload, &["issue", "created_at"])?,
        updated_at: stamp(payload, &["issue", "updated_at"])?,
        closed_at,
    };
    let action = text(payload, &["action"])?;
    let action = match payload.get("comment") {
        Some(_) if action == "created" => Action::Commented(Comment {
            login: text(payload, &["comment", "user", "login"])?,
            standing: Standing::of(&text(payload, &["comment", "author_association"])?),
            body: text(payload, &["comment", "body"])?,
            at: stamp(payload, &["comment", "created_at"])?,
        }),
        Some(_) => Action::Other(format!("comment {action}")),
        None => match action.as_str() {
            "opened" => Action::Opened,
            "edited" => Action::Edited {
                title_changed: field(payload, &["changes", "title"]).is_some(),
            },
            "closed" => Action::Closed,
            "reopened" => Action::Reopened,
            _ => Action::Other(action),
        },
    };
    Ok(Delivery {
        issue,
        action,
        pull_request: field(payload, &["issue", "pull_request"]).is_some(),
    })
}

// --- the /price grammar ------------------------------------------------------

/// The `/price` line of a text, if it has one: the first line that, trimmed
/// of whitespace and backticks, starts with the word `/price`.
pub fn price_line(text: &str) -> Option<&str> {
    text.lines()
        .map(|line| line.trim().trim_matches('`').trim())
        .find(|line| line.split_whitespace().next() == Some("/price"))
}

fn percent(word: Option<&str>, what: &str) -> Result<u8, Error> {
    let word = word.ok_or_else(|| Error::usage(format!("/price: {what} needs a number")))?;
    let n: u8 = word
        .trim_end_matches('%')
        .parse()
        .map_err(|_| Error::usage(format!("/price: {what} must be 0–100, got {word:?}")))?;
    if n > 100 {
        return Err(Error::usage(format!(
            "/price: {what} must be 0–100, got {n}"
        )));
    }
    Ok(n)
}

/// A `/price` line as the price it states.
///
/// ```text
/// /price 40
/// /price 30 --deadline 2026-10-15 [--end 5] [--lead-up 3]
/// /price ref gh-7
/// /price accept
/// ```
///
/// # Errors
///
/// Anything else is a refusal that says what was expected — the workflow posts
/// it back to the maintainer who wrote the line.
pub fn parse_price(line: &str) -> Result<Price, Error> {
    let mut words = line.split_whitespace();
    if words.next() != Some("/price") {
        return Err(Error::usage("/price: a price line starts with /price"));
    }
    let first = words
        .next()
        .ok_or_else(|| Error::usage("/price: expected a number, `ref <item>` or `accept`"))?;
    let price = match first {
        "accept" => Price::Accept,
        "ref" => {
            let target = words
                .next()
                .ok_or_else(|| Error::usage("/price ref: name the item, e.g. gh-7"))?;
            Price::Ref(TodoId::new(target)?)
        }
        number => {
            let start = percent(Some(number), "the price")?;
            let mut day = None;
            let mut end = PriceArgs::DEFAULT_END;
            let mut lead_up_days = PriceArgs::DEFAULT_LEAD_UP_DAYS;
            while let Some(flag) = words.next() {
                match flag {
                    "--deadline" => day = words.next().map(str::to_owned),
                    "--end" => end = percent(words.next(), "--end")?,
                    "--lead-up" => {
                        let word = words.next().unwrap_or_default();
                        lead_up_days = word.parse().map_err(|_| {
                            Error::usage(format!("/price: --lead-up takes days, got {word:?}"))
                        })?;
                    }
                    other => {
                        return Err(Error::usage(format!(
                            "/price: unexpected {other:?}; expected --deadline, --end or --lead-up"
                        )))
                    }
                }
            }
            match day {
                Some(day) => Price::Deadline {
                    start,
                    day,
                    end,
                    lead_up_days,
                },
                None if end != PriceArgs::DEFAULT_END
                    || lead_up_days != PriceArgs::DEFAULT_LEAD_UP_DAYS =>
                {
                    return Err(Error::usage("/price: --end and --lead-up need --deadline"))
                }
                None => Price::Flat(start),
            }
        }
    };
    if let Some(extra) = words.next() {
        return Err(Error::usage(format!("/price: unexpected {extra:?}")));
    }
    Ok(price)
}

/// A stated price as the §7 term it names; `Accept` names none by itself.
fn term_of(price: &Price) -> Result<Option<Term>, Error> {
    let args = |priority, deadline, start, end, lead_up_days| PriceArgs {
        priority,
        deadline,
        start,
        end,
        lead_up_days,
    };
    match price {
        Price::Flat(n) => price::term_of(&args(
            Some(*n),
            None,
            PriceArgs::DEFAULT_START,
            PriceArgs::DEFAULT_END,
            PriceArgs::DEFAULT_LEAD_UP_DAYS,
        )),
        Price::Deadline {
            start,
            day,
            end,
            lead_up_days,
        } => price::term_of(&args(None, Some(day.clone()), *start, *end, *lead_up_days)),
        Price::Ref(todo) => Ok(Some(mk_ref(todo.as_str().to_owned())?)),
        Price::Accept => Ok(None),
    }
}

// --- the events a delivery means ---------------------------------------------

/// A GitHub login as an actor: lowercased, and prefixed `gh-` when it starts
/// with a digit, since an actor starts with a letter. The two actors this
/// module writes as are refused, so no account can sign as the mirror.
pub fn actor_of_login(login: &str) -> Result<String, Error> {
    let lower = login.to_ascii_lowercase();
    let actor = if lower.starts_with(|c: char| c.is_ascii_digit()) {
        format!("gh-{lower}")
    } else {
        lower
    };
    if actor == MIRROR || actor == BOT {
        return Err(Error::usage(format!(
            "/price: {login:?} would sign as {actor:?}, which only the workflows write as"
        )));
    }
    Ok(actor)
}

/// The item's content record: the title as its body, the issue as its source.
fn record(issue: &Issue, at: Datetime, spec: Option<Term>) -> Result<TodoEvent<Todo>, Error> {
    let source = mk_source(
        &issue.author,
        &issue.title,
        issue.created_at,
        "",
        "",
        &issue.url,
    );
    Ok(mk_authored(
        &issue.todo(),
        at,
        MIRROR,
        "todo",
        issue.created_at,
        &issue.title,
        spec,
        Vec::new(),
        CATEGORY,
        "",
        "",
        Some(source),
        Vec::new(),
        Vec::new(),
        "",
    )?)
}

/// What the store already says, as far as a delivery needs to know.
pub(crate) struct Known<'a> {
    dag: &'a Dag<TodoEvent<Todo>>,
    events: Vec<TodoEvent<Todo>>,
    state: Folded<TodoEvent<Todo>>,
    genesis: Genesis,
}

impl<'a> Known<'a> {
    fn of(dag: &'a Dag<TodoEvent<Todo>>) -> Result<Known<'a>, Error> {
        let nodes = dag.nodes()?;
        Ok(Known {
            dag,
            events: nodes
                .iter()
                .filter_map(|node| node.event().cloned())
                .collect(),
            state: registers::fold(&nodes),
            genesis: dag.geneses().first().and_then(|genesis| dag.key(genesis)),
        })
    }

    pub(crate) fn has_item(&self, todo: &str) -> bool {
        self.events
            .iter()
            .any(|event| matches!(event, TodoEvent::Created(e) if e.todo.as_str() == todo))
    }

    /// The acceptance `actor` already wrote for `todo` at `at`, if any.
    fn accepted(&self, todo: &str, actor: &str, at: Datetime) -> Option<&TodoEvent<Todo>> {
        self.events.iter().find(|event| {
            matches!(event, TodoEvent::SpecRevised(e)
                if e.todo.as_str() == todo
                    && e.actor.as_str() == actor
                    && e.at == at
                    && e.note == ACCEPTED)
        })
    }

    /// The pricing bot's latest proposal for `todo` dated at or before `at`,
    /// in the store's causal order — so accepting is a function of the
    /// comment's instant, and a replay accepts the same proposal again.
    fn proposal(&self, todo: &str, at: Datetime) -> Option<&Term> {
        self.events.iter().rev().find_map(|event| match event {
            TodoEvent::SpecRevised(e)
                if e.todo.as_str() == todo && e.actor.as_str() == BOT && e.at <= at =>
            {
                Some(&e.spec)
            }
            _ => None,
        })
    }
}

/// Why a delivery appends nothing, said in the outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    PullRequest,
    Uninteresting(String),
    NotAPrice,
    Outsider(String),
    BodyOnlyEdit,
    /// A pull request whose description has no `Completes:` line.
    CompletesNothing,
    /// A pull request that has not merged, with the items that its merge
    /// will complete.
    Unmerged(BTreeSet<TodoId>),
}

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Skip::PullRequest => write!(f, "a pull request, not mirrored"),
            Skip::Uninteresting(action) => write!(f, "{action}: nothing the mirror records"),
            Skip::NotAPrice => write!(f, "a comment that is not a /price"),
            Skip::Outsider(login) => {
                write!(f, "a /price from {login}, who is not a maintainer: ignored")
            }
            Skip::BodyOnlyEdit => write!(f, "an edit that left the title alone"),
            Skip::CompletesNothing => write!(
                f,
                "a pull request with no `{}` line, which completes nothing",
                completes::WORD
            ),
            Skip::Unmerged(items) => write!(
                f,
                "a pull request that has not merged; its merge completes {}",
                items
                    .iter()
                    .map(TodoId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// The events a delivery means against what the store holds: `Err(Skip)`
/// when it means none.
fn meant(
    delivery: &Delivery,
    proposal: Option<&str>,
    known: &Known<'_>,
) -> Result<Result<Vec<TodoEvent<Todo>>, Skip>, Error> {
    let issue = &delivery.issue;
    let todo = issue.todo();
    if delivery.pull_request {
        return Ok(Err(Skip::PullRequest));
    }

    // The item first, whatever the action: an issue opened before the mirror
    // existed is created by the first delivery that mentions it, from the
    // issue as that delivery carries it.
    let mut events = Vec::new();
    if !known.has_item(&todo) {
        let neutral = term_of(&Price::Flat(NEUTRAL))?;
        events.push(mk_created(
            &todo,
            issue.created_at,
            MIRROR,
            &issue.title,
            "",
        )?);
        events.push(record(issue, issue.created_at, neutral)?);
    }

    if let Some(reply) = proposal {
        return proposed(issue, reply, known, &events).map(|mut more| {
            events.append(&mut more);
            Ok(events)
        });
    }

    let more = match &delivery.action {
        Action::Opened => Vec::new(),
        // A retitle is a content revision carrying no spec, so it keeps the
        // price in force. Written even when the item was created just above:
        // what a delivery means must not depend on whether it came first, or a
        // replay would mean something else.
        Action::Edited {
            title_changed: true,
        } => vec![record(issue, issue.updated_at, None)?],
        Action::Edited {
            title_changed: false,
        } => {
            return Ok(if events.is_empty() {
                Err(Skip::BodyOnlyEdit)
            } else {
                Ok(events)
            })
        }
        Action::Closed => {
            let at = issue.closed_at.unwrap_or(issue.updated_at);
            vec![mk_completed(&todo, at, MIRROR, "")?]
        }
        Action::Reopened => vec![mk_reopened(&todo, issue.updated_at, MIRROR, "")?],
        Action::Commented(comment) => match priced(issue, comment, known)? {
            Ok(event) => vec![event],
            Err(skip) => return Ok(Err(skip)),
        },
        Action::Other(action) => {
            return Ok(if events.is_empty() {
                Err(Skip::Uninteresting(action.clone()))
            } else {
                Ok(events)
            })
        }
    };
    events.extend(more);
    Ok(Ok(events))
}

/// A comment, as the repricing it is — or why it is none.
///
/// THE STANDING IS ASKED BEFORE THE GRAMMAR. An outsider's `/price` is
/// ignored whatever it says, so a malformed one is never answered and a
/// stranger cannot make the workflow speak.
fn priced(
    issue: &Issue,
    comment: &Comment,
    known: &Known<'_>,
) -> Result<Result<TodoEvent<Todo>, Skip>, Error> {
    let Some(line) = comment
        .body
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| line.split_whitespace().next() == Some("/price"))
    else {
        return Ok(Err(Skip::NotAPrice));
    };
    if comment.standing != Standing::Maintainer {
        return Ok(Err(Skip::Outsider(comment.login.clone())));
    }
    let actor = actor_of_login(&comment.login)?;
    let todo = issue.todo();
    let price = parse_price(line)?;
    let (spec, note) = match &price {
        Price::Accept => {
            // AN ACCEPT IS APPLIED ONCE, WHATEVER IT WOULD RESOLVE TO NOW. It
            // reads the store, so a replay after a later-dated proposal arrived
            // could resolve differently; the comment's own instant is its key.
            if let Some(done) = known.accepted(&todo, &actor, comment.at) {
                return Ok(Ok(done.clone()));
            }
            let spec = known.proposal(&todo, comment.at).cloned().ok_or_else(|| {
                Error::usage(format!(
                    "/price accept: no proposal from {BOT} for {todo} yet"
                ))
            })?;
            (spec, ACCEPTED.to_owned())
        }
        other => {
            require_target(other, &todo, known)?;
            (
                term_of(other)?.expect("only Accept names no term"),
                String::new(),
            )
        }
    };
    Ok(Ok(mk_spec_revised(&todo, comment.at, &actor, spec, &note)?))
}

/// A reference must name an item the store holds, and not the item itself.
fn require_target(price: &Price, todo: &str, known: &Known<'_>) -> Result<(), Error> {
    if let Price::Ref(target) = price {
        if target.as_str() == todo {
            return Err(Error::usage(format!(
                "/price ref: {todo} cannot wait on itself"
            )));
        }
        if !known.has_item(target.as_str()) {
            return Err(Error::usage(format!(
                "/price ref: no item {:?} in the roadmap",
                target.as_str()
            )));
        }
    }
    Ok(())
}

/// The pricing step's reply, as the claim it records: its `/price` line, as
/// [`BOT`], with its rationale as the note.
///
/// DATED NO EARLIER THAN WHAT IT IS WRITTEN ON. A reader who names the bot
/// untrusted holds its events to §3's clock rule — never dated behind an
/// ancestor — and the model takes minutes, in which this item may be
/// repriced. So the claim is dated at the latest instant among the issue's
/// last update, everything its deps rest on, and the events this delivery
/// writes before it. A replay after a maintainer's repricing would be dated
/// past it, so a replay is recognised by what it claims: a claim of the same
/// price and note for this item, dated at or after this delivery's instant,
/// is this one already applied.
fn proposed(
    issue: &Issue,
    reply: &str,
    known: &Known<'_>,
    before: &[TodoEvent<Todo>],
) -> Result<Vec<TodoEvent<Todo>>, Error> {
    let line =
        price_line(reply).ok_or_else(|| Error::usage("proposal: the reply has no /price line"))?;
    let price = parse_price(line)?;
    if price == Price::Accept {
        return Err(Error::usage(
            "proposal: only a maintainer accepts a proposal",
        ));
    }
    let todo = issue.todo();
    require_target(&price, &todo, known)?;
    let spec = term_of(&price)?.expect("only Accept names no term");
    let note = rationale_of(reply);

    let applied = known.events.iter().any(|event| {
        matches!(event, TodoEvent::SpecRevised(e)
            if e.todo.as_str() == todo
                && e.actor.as_str() == BOT
                && e.at >= issue.updated_at
                && e.spec == spec
                && e.note == note)
    });
    if applied {
        return Ok(Vec::new());
    }
    let draft = mk_spec_revised(&todo, issue.updated_at, BOT, spec.clone(), &note)?;
    let deps = registers::deps_for(&known.state, &known.genesis, &draft)?;
    let at = known
        .dag
        .closure(deps)
        .iter()
        .filter_map(|name| known.dag.get(name).and_then(Envelope::event))
        .chain(before)
        .map(TodoEvent::at)
        .fold(issue.updated_at, std::cmp::max);
    Ok(vec![mk_spec_revised(&todo, at, BOT, spec, &note)?])
}

/// The text after the reply's `Rationale:` label (bold or not), cut to
/// [`NOTE_LIMIT`] characters; empty when there is no label.
fn rationale_of(reply: &str) -> String {
    let rest = reply
        .lines()
        .map(|line| line.trim().trim_start_matches(['*', '#', '-', ' ']))
        .find_map(|line| line.strip_prefix("Rationale:"))
        .unwrap_or_default();
    let rest = rest.trim_start().strip_prefix("**").unwrap_or(rest).trim();
    rest.chars().take(NOTE_LIMIT).collect()
}

/// The events a delivery means against a store holding `dag`.
///
/// # Errors
///
/// A maintainer's `/price` that does not parse, names an unknown item, or
/// accepts a proposal there is none of; a proposal with no `/price` line.
pub fn plan(
    delivery: &Delivery,
    proposal: Option<&str>,
    dag: &Dag<TodoEvent<Todo>>,
) -> Result<Result<Vec<TodoEvent<Todo>>, Skip>, Error> {
    meant(delivery, proposal, &Known::of(dag)?)
}

/// `prodrome-github PAYLOAD.json [--proposal FILE] [--store DIR]`.
#[derive(Debug, clap::Parser)]
#[command(name = "prodrome-github", version, about)]
pub struct Cli {
    /// The payload, as a workflow finds it at `$GITHUB_EVENT_PATH`.
    #[arg(value_name = "PAYLOAD.json")]
    pub payload: std::path::PathBuf,
    /// The pricing step's reply: record its `/price` line as a proposal by
    /// `pricing-bot`, a claim, instead of mirroring the delivery.
    #[arg(long, value_name = "FILE")]
    pub proposal: Option<std::path::PathBuf>,
    /// The store, as `prodrome --store` names it: `./roadmap` when that
    /// directory exists, and the current directory otherwise.
    #[arg(long, value_name = "DIR")]
    pub store: Option<std::path::PathBuf>,
}

/// Apply one GitHub webhook delivery (`issues`, `issue_comment` or
/// `pull_request`) to the store: an issue becomes item `gh-<number>`, a
/// maintainer's `/price` comment reprices it, and a merged pull request
/// completes the items that its `Completes:` lines name. Deterministic and
/// offline, and a delivery already applied appends nothing.
///
/// # Errors
/// A payload or reply that does not parse, and the store's own refusals.
pub fn run(cli: &Cli) -> Result<Outcome, Error> {
    let store = Store::new(
        store_root(cli.store.as_deref()),
        prodrome::policy::Untrusted::none(),
    );
    apply(&store, &cli.payload, cli.proposal.as_deref())
}

/// Read the payload (and the reply, for a proposal), and apply it.
///
/// # Errors
///
/// A payload or reply that does not parse, a proposal beside a pull request,
/// and the store's own refusals.
pub fn apply(store: &Store, payload: &Path, proposal: Option<&Path>) -> Result<Outcome, Error> {
    let payload = read_json(payload)?;
    match (payload.get("pull_request"), proposal) {
        (Some(pull), None) => apply_pull(store, &completes::pull_of(pull)?),
        (Some(_), Some(_)) => Err(Error::usage(
            "--proposal: a proposal prices an issue, and this payload is a pull request",
        )),
        (None, proposal) => {
            let delivery = delivery_of(&payload)?;
            let reply = proposal.map(read_text).transpose()?;
            apply_delivery(store, &delivery, reply.as_deref())
        }
    }
}

/// Plan one delivery against the store and append what it means: the digest
/// of each event, one per line, or why there are none. A replay answers the
/// digests it answered the first time, and writes nothing.
pub fn apply_delivery(
    store: &Store,
    delivery: &Delivery,
    proposal: Option<&str>,
) -> Result<Outcome, Error> {
    append(store, plan(delivery, proposal, &*store.dag()?)?)
}

/// Plan one pull request against the store and append the completions that
/// its merge means, as [`apply_delivery`] does for an issue. A pull request
/// that has not merged appends nothing, and the outcome names the items that
/// its merge will complete.
///
/// # Errors
///
/// A `Completes:` line that names an item the store does not hold, and the
/// store's own refusals.
pub fn apply_pull(store: &Store, pull: &completes::Pull) -> Result<Outcome, Error> {
    let dag = store.dag()?;
    append(store, completes::meant(pull, &Known::of(&dag)?)?)
}

/// Append what a delivery means: the digest of each event, one per line, or
/// why there are none.
fn append(store: &Store, meant: Result<Vec<TodoEvent<Todo>>, Skip>) -> Result<Outcome, Error> {
    match meant {
        Err(skip) => Ok(Outcome::said(format!("nothing to append: {skip}"))),
        Ok(meant) if meant.is_empty() => Ok(Outcome::said("nothing to append: already applied")),
        Ok(meant) => {
            let mut written = Vec::with_capacity(meant.len());
            for event in meant {
                written.push(store.append(event)?.as_str().to_owned());
            }
            Ok(Outcome::said(written.join("\n")))
        }
    }
}

fn read_text(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|e| {
        Error::Store(ProdromeError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })
    })
}

fn read_json(path: &Path) -> Result<Value, Error> {
    serde_json::from_str(&read_text(path)?)
        .map_err(|e| Error::usage(format!("payload {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use prodrome::fpl::print_term;

    fn term(line: &str) -> String {
        print_term(
            &term_of(&parse_price(line).expect("parses"))
                .expect("a term")
                .expect("some"),
        )
    }

    #[test]
    fn a_number_is_flat() {
        assert_eq!(parse_price("/price 40").expect("parses"), Price::Flat(40));
        assert_eq!(term("/price 40"), "Flat(value=0.4)");
        assert_eq!(term("/price 40%"), "Flat(value=0.4)");
    }

    #[test]
    fn a_deadline_is_a_decay() {
        assert_eq!(
            term("/price 30 --deadline 2026-10-15"),
            "Decay(start=0.3, end=0.05, end_date=datetime(2026, 10, 15, 17, 0, 0), lead_up=timedelta(days=3), start_date=None)"
        );
        assert_eq!(
            term("/price 30 --deadline 2026-10-15 --end 10 --lead-up 14"),
            "Decay(start=0.3, end=0.1, end_date=datetime(2026, 10, 15, 17, 0, 0), lead_up=timedelta(days=14), start_date=None)"
        );
    }

    #[test]
    fn a_ref_depends_on_another_item() {
        assert_eq!(term("/price ref gh-7"), "Ref(todo='gh-7')");
    }

    #[test]
    fn accept_is_its_own_word() {
        assert_eq!(parse_price("/price accept").expect("parses"), Price::Accept);
    }

    #[test]
    fn malformed_prices_are_refused() {
        for line in [
            "/price",
            "/price soon",
            "/price 101",
            "/price 40 extra",
            "/price 40 --end 3",
            "/price 30 --deadline tomorrow",
            "/price 99 --deadline 2026-10-15",
            "/price ref",
            "/price ref GH 7",
            "/price accept now",
        ] {
            let refused = parse_price(line).and_then(|price| term_of(&price).map(|_| ()));
            assert!(refused.is_err(), "{line} should be refused");
        }
    }

    #[test]
    fn a_price_line_is_found_in_a_reply() {
        let reply =
            "Here is my read.\n\n`/price 45`\n\n**Rationale:** a real bug with a workaround";
        assert_eq!(price_line(reply), Some("/price 45"));
        assert_eq!(rationale_of(reply), "a real bug with a workaround");
        assert_eq!(rationale_of("- Rationale: plain"), "plain");
        assert_eq!(rationale_of("no label"), "");
        assert_eq!(price_line("no price at all"), None);
        assert_eq!(price_line("/pricey 4"), None);
    }

    #[test]
    fn a_login_becomes_an_actor() {
        assert_eq!(actor_of_login("BMabsout").expect("ok"), "bmabsout");
        assert_eq!(actor_of_login("42wizard").expect("ok"), "gh-42wizard");
        assert!(actor_of_login("Pricing-Bot").is_err());
        assert!(actor_of_login("github-mirror").is_err());
    }

    #[test]
    fn only_three_associations_may_price() {
        for association in ["OWNER", "MEMBER", "COLLABORATOR"] {
            assert_eq!(Standing::of(association), Standing::Maintainer);
        }
        for association in [
            "CONTRIBUTOR",
            "FIRST_TIMER",
            "NONE",
            "MANNEQUIN",
            "owner",
            "",
        ] {
            assert_eq!(Standing::of(association), Standing::Outsider);
        }
    }

    #[test]
    fn a_timestamp_must_be_utc() {
        assert_eq!(
            price::iso(instant_of("2026-09-27T12:34:56Z").expect("utc")),
            "2026-09-27T12:34:56"
        );
        assert!(instant_of("2026-09-27T12:34:56+02:00").is_err());
    }
}
