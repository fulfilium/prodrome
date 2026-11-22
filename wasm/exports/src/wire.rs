//! The JSON shapes that cross the boundary, and nothing else.
//!
//! The exports are functions of JSON text; this is what their arguments and
//! their answers LOOK like, written down once so that a page's own API module's
//! mirror has a single thing to mirror. Everything is JSON text: a string in,
//! a string out. That is deliberate — `serde-wasm-bindgen` would let a JS
//! object cross directly and cost a second serialisation format to keep in
//! step with the wire the server already speaks, and this crate's whole reason
//! to exist is that there is ONE reading of the data.
//!
//! ONE INSTANT SHAPE IN AND OUT: `fpl::iso` (`2026-09-06T11:32:07`), what
//! `datetime.isoformat()` prints and what every `at` on the JSON API carries.
//! So `fold`'s `env` can be handed straight back to `fulfillment`,
//! and `fold`'s `history` straight back to `series_knots`, with
//! nothing rewritten in between — a translation step is where a second reading
//! grows.
//!
//! A RECORD'S FIELDS ARE THE HOST'S, so this file does not name one. §4's
//! record kind is `KIND(todo, at, actor, <the payload's fields>)`, and
//! [`json_record`] serialises exactly that: the three the core owns, then the
//! payload's own `fields()` as a JSON object keyed by field name, each a §2
//! literal mapped by [`json_literal`]. A browser therefore reads the same
//! names its store holds, whatever payload the module was built with — and
//! this crate stops having an opinion about what a body is.
//!
//! `json_entry`'s `at` is `isoformat(" ")` and used to be the page's to spell;
//! since §6.7 it is `Entry::at`, in the core — one spelling, in one place.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use prodrome::dag::Dag;
use prodrome::event::{Authored, Hash, TodoEvent, TodoId};
use prodrome::fpl::{self, Candidates, Closed, Env, Instant, Outcome};
use prodrome::literal;
use prodrome::observe::{NextChange, Observation, Rounding};
use prodrome::payload::Payload;
use prodrome::policy::{Policy, Proven, Standing, Untrusted};
use prodrome::schedule::Schedule;
use prodrome::schema::Schema;
use prodrome::sign::PublicKey;
use prodrome::term::Term;
use prodrome::view::Entry;
use serde::Deserialize;
use serde_json::{json, Map, Value};

/// One stored object as the browser holds it: the name it claims and the exact
/// bytes that name is a hash of. `/api/chain`'s `hash` and `literal`.
#[derive(Debug, Deserialize)]
pub struct ObjectIn {
    pub hash: String,
    pub text: String,
}

/// Everything this crate refuses with. A `String` and not an enum: every one
/// of these ends up as a JS exception message read by a person, the callers
/// never branch on the reason, and an enum whose only observation is its
/// `Display` is a ceremony with no reader.
pub type Refusal = String;

pub fn parse_objects(json_text: &str) -> Result<Vec<ObjectIn>, Refusal> {
    serde_json::from_str(json_text).map_err(|e| format!("objects: expected [{{hash, text}}] ({e})"))
}

/// The deployment's STANDING policy (§5), as the browser was told it. It
/// arrives from `/api/chain` rather than being assumed here: WHICH actors a
/// host stands behind is instance knowledge, and a core that guessed it would
/// fold a different chain than the server did while claiming to fold the same
/// one.
///
/// THE WIRE IS UNCHANGED. `untrusted` is still a JSON array of actor names;
/// since 0.3 the core takes a `policy::Policy` rather than that array, and this
/// is where the array becomes one — `Untrusted`, the reference policy, which is
/// the rule this argument always meant.
pub fn parse_untrusted(json_text: &str) -> Result<Untrusted, Refusal> {
    let names: Vec<String> = serde_json::from_str(json_text)
        .map_err(|e| format!("untrusted: expected a list of actor names ({e})"))?;
    let mut actors = Vec::with_capacity(names.len());
    for name in names {
        actors.push(prodrome::event::Actor::new(name).map_err(|e| format!("untrusted: {e}"))?);
    }
    Ok(Untrusted::of(actors))
}

/// A policy as the browser was told it: the reference policy alone (a JSON
/// array of actor names, the wire as it always was), or, as `{untrusted,
/// roots}`, that policy held to the actors' keys, registered by the root
/// keys `roots` (§5's policy that requires signatures).
#[derive(Debug, Clone)]
pub enum WirePolicy {
    Roster(Untrusted),
    Proven(Proven<Untrusted>),
}

impl<E: Schema> Policy<E> for WirePolicy {
    fn standing(&self, object: &Hash, event: &E) -> Standing {
        match self {
            WirePolicy::Roster(policy) => policy.standing(object, event),
            WirePolicy::Proven(policy) => policy.standing(object, event),
        }
    }

    fn confirms(&self, object: &Hash, event: &E) -> bool {
        match self {
            WirePolicy::Roster(policy) => policy.confirms(object, event),
            WirePolicy::Proven(policy) => policy.confirms(object, event),
        }
    }

    fn at(&self, history: &Dag<E>) -> Option<Self> {
        match self {
            WirePolicy::Roster(_) => None,
            WirePolicy::Proven(policy) => policy.at(history).map(WirePolicy::Proven),
        }
    }
}

/// [`WirePolicy`] from its JSON, at the history `history`.
///
/// # Errors
///
/// Neither shape, or an actor or a key that is not one.
pub fn parse_policy<E: Schema>(json_text: &str, history: &Dag<E>) -> Result<WirePolicy, Refusal> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Held {
        untrusted: Value,
        roots: Value,
    }
    let policy = match serde_json::from_str::<Held>(json_text) {
        Ok(Held { untrusted, roots }) => WirePolicy::Proven(Proven::new(
            parse_untrusted(&untrusted.to_string())?,
            parse_keys("roots", &roots.to_string())?,
            history,
        )),
        Err(_) => WirePolicy::Roster(parse_untrusted(json_text)?),
    };
    Ok(policy)
}

/// A JSON array of public keys, each checked as one.
///
/// # Errors
///
/// Not an array of strings, or a string that is not a key.
pub fn parse_keys(field: &str, json_text: &str) -> Result<Vec<PublicKey>, Refusal> {
    let keys: Vec<String> = serde_json::from_str(json_text)
        .map_err(|e| format!("{field}: expected a list of public keys ({e})"))?;
    keys.into_iter()
        .map(|key| PublicKey::new(key).map_err(|e| format!("{field}: {e}")))
        .collect()
}

/// A JSON array of object names, each checked as one.
pub fn parse_names(field: &str, json_text: &str) -> Result<Vec<Hash>, Refusal> {
    let names: Vec<String> = serde_json::from_str(json_text)
        .map_err(|e| format!("{field}: expected a list of object names ({e})"))?;
    names
        .into_iter()
        .map(|name| Hash::new(name).map_err(|e| format!("{field}: {e}")))
        .collect()
}

pub fn parse_instant(field: &str, text: &str) -> Result<Instant, Refusal> {
    fpl::parse_iso(text).map_err(|e| format!("{field}: {e}"))
}

pub fn parse_moment(field: &str, text: Option<String>) -> Result<Option<Instant>, Refusal> {
    text.map(|t| parse_instant(field, &t)).transpose()
}

pub fn parse_term(field: &str, json_text: &str) -> Result<Term, Refusal> {
    let value: Value =
        serde_json::from_str(json_text).map_err(|e| format!("{field}: not JSON ({e})"))?;
    crate::json::from_json(&value).map_err(|e| format!("{field}: {e}"))
}

/// `{"<todo>": <term>}` — what `link` binds each `ref` to, every
/// term in the JSON shape above.
pub fn parse_specs(json_text: &str) -> Result<BTreeMap<String, Term>, Refusal> {
    specs_of(serde_json::from_str(json_text).map_err(|e| format!("specs: {e}"))?)
}

fn specs_of(raw: BTreeMap<String, Value>) -> Result<BTreeMap<String, Term>, Refusal> {
    raw.into_iter()
        .map(|(todo, term)| {
            crate::json::from_json(&term)
                .map(|term| (todo.clone(), term))
                .map_err(|e| format!("specs.{todo}: {e}"))
        })
        .collect()
}

/// `[{"genesis": "<name>" | null, "env": <env>, "functions": <specs>}]`,
/// the `prices` answer's `prices`: the set of prodromes a qualified
/// reference reads (SPEC §7.2), each member by its genesis, its entities'
/// functions and the environment its terms read. A legacy prodrome
/// (`genesis` `null`) has no genesis for a qualifier to name, so no
/// reference reaches it and it is not a member. A member named twice is
/// refused, never merged.
///
/// # Errors
///
/// Text that is not that shape, a name that is not a genesis's, or one
/// genesis named twice.
pub fn parse_stores(json_text: &str) -> Result<fpl::Stores, Refusal> {
    let raw: Vec<MemberIn> = serde_json::from_str(json_text).map_err(|e| format!("stores: {e}"))?;
    let mut members = BTreeMap::new();
    for MemberIn {
        genesis,
        env,
        functions,
    } in raw
    {
        let Some(genesis) = genesis else { continue };
        let name = Hash::new(genesis.clone()).map_err(|e| format!("stores: {e}"))?;
        let member = fpl::Member {
            specs: specs_of(functions).map_err(|e| format!("stores.{genesis}.{e}"))?,
            env: env_of(env).map_err(|e| format!("stores.{genesis}.{e}"))?,
        };
        if members.insert(name, member).is_some() {
            return Err(format!("stores: the genesis {genesis} is named twice"));
        }
    }
    Ok(members.into_iter().collect())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberIn {
    genesis: Option<String>,
    env: EnvIn,
    functions: BTreeMap<String, Value>,
}

/// A term the evaluator can read: one with no `ref` in it (SPEC §7).
pub fn parse_closed(field: &str, json_text: &str) -> Result<Closed, Refusal> {
    Closed::of(parse_term(field, json_text)?)
        .ok_or_else(|| format!("{field}: holds a ref; link it first"))
}

/// `{"levels": n, "rounding": "down" | "nearest" | "up"}`: what a view
/// observes of a fulfillment (§7), `[0, 1]` in `n ≥ 1` steps read by the
/// rounding. A whole percent is `{"levels": 100, "rounding": "nearest"}`.
/// Any other key is refused, never ignored.
///
/// # Errors
///
/// Text that is not that shape, or no steps.
pub fn parse_observation(json_text: &str) -> Result<Observation, Refusal> {
    let raw: ObservationIn =
        serde_json::from_str(json_text).map_err(|e| format!("observation: {e}"))?;
    let levels = NonZeroU32::new(raw.levels)
        .ok_or_else(|| "observation.levels: must be at least 1".to_owned())?;
    let rounding = match raw.rounding {
        RoundingIn::Down => Rounding::Down,
        RoundingIn::Nearest => Rounding::Nearest,
        RoundingIn::Up => Rounding::Up,
    };
    Ok(Observation::new(levels, rounding))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservationIn {
    levels: u32,
    rounding: RoundingIn,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RoundingIn {
    Down,
    Nearest,
    Up,
}

/// `{"at": "<iso>" | null, "exact": bool}`: when an observed value next
/// changes, `null` for never; `exact` where `at` is its next step, and not
/// where it is a bound that is never later than the step.
pub fn json_next_change(next: NextChange) -> Value {
    json!({ "at": next.at.map(fpl::iso), "exact": next.exact })
}

/// `{"outcomes": {"<name>": {"kind": "Completed" | "Cancelled", "at":
/// "<iso>"}}, "tended": {"<todo>": ["<iso>", …]}}` — §7's environment, its two
/// halves as the core holds them. A conflict's outcome is the array of its
/// candidates, `null` for an open one. An outcome has the same two fields
/// `conformance/fpl.py`'s `Bound(...)` carries, so a vector's environment
/// reaches this without a translation step that could disagree. A half left
/// out is empty, so `{}` is the environment that binds nothing; any other key
/// is refused, never ignored.
pub fn parse_env(json_text: &str) -> Result<fpl::Env, Refusal> {
    env_of(serde_json::from_str(json_text).map_err(|e| format!("env: {e}"))?)
}

fn env_of(raw: EnvIn) -> Result<fpl::Env, Refusal> {
    let mut env = fpl::Env::new();
    for (name, bound) in raw.outcomes {
        env.outcomes.insert(name, bound.candidates()?);
    }
    env.tended = parse_tended("env.tended", raw.tended)?;
    Ok(env)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvIn {
    #[serde(default)]
    outcomes: BTreeMap<String, Bound>,
    #[serde(default)]
    tended: BTreeMap<String, Vec<String>>,
}

/// Per todo, its tendings as ISO instants — a set, so order and repeats on the
/// wire mean nothing.
fn parse_tended(
    field: &str,
    raw: BTreeMap<String, Vec<String>>,
) -> Result<BTreeMap<String, BTreeSet<Instant>>, Refusal> {
    raw.into_iter()
        .map(|(todo, instants)| {
            let set = instants
                .iter()
                .map(|at| parse_instant(field, at))
                .collect::<Result<_, _>>()?;
            Ok((todo, set))
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Bound {
    One(EnvEntry),
    Many(Vec<Option<EnvEntry>>),
}

impl Bound {
    fn candidates(&self) -> Result<Candidates, Refusal> {
        match self {
            Bound::One(entry) => Ok([Some(entry.outcome()?)].into()),
            Bound::Many(entries) => entries
                .iter()
                .map(|entry| entry.as_ref().map(EnvEntry::outcome).transpose())
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct EnvEntry {
    pub kind: String,
    pub at: String,
}

impl EnvEntry {
    fn outcome(&self) -> Result<Outcome, Refusal> {
        let at = parse_instant("env.at", &self.at)?;
        match self.kind.as_str() {
            "Completed" => Ok(Outcome::Completed(at)),
            "Cancelled" => Ok(Outcome::Cancelled(at)),
            other => Err(format!(
                "env.kind must be \"Completed\" or \"Cancelled\", got {other:?}"
            )),
        }
    }
}

/// The environment as a FUNCTION OF TIME (§6.5), as `fold`'s `history` emits
/// it and `series_knots` reads it back: `bindings`, per todo, its outcome at
/// each instant the reading changes, ascending — `null` for open, and a
/// conflict as its candidates, as in [`parse_env`]; and `tended`, per todo,
/// every tending, which as a grow-only set is its own history. The same
/// two-halves rule as [`parse_env`]: either may be left out, and nothing else
/// is admitted.
///
/// It is a separate argument from `env` and not a convenience over it: a knot
/// at a past instant must be evaluated against the environment AS OF that
/// instant (`view.series_of` does exactly this), and a single `env` snapshot
/// would bind a dependency before it was completed.
#[derive(Debug, Deserialize)]
struct HistoryEntry {
    at: String,
    binding: Option<Bound>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryIn {
    #[serde(default)]
    bindings: BTreeMap<String, Vec<HistoryEntry>>,
    #[serde(default)]
    tended: BTreeMap<String, Vec<String>>,
}

pub struct History {
    bindings: BTreeMap<String, Schedule<Candidates>>,
    tended: BTreeMap<String, BTreeSet<Instant>>,
}

impl History {
    pub fn parse(json_text: &str) -> Result<History, Refusal> {
        let raw: HistoryIn =
            serde_json::from_str(json_text).map_err(|e| format!("history: {e}"))?;
        let mut bindings = BTreeMap::new();
        for (todo, entries) in raw.bindings {
            let mut timeline = Vec::with_capacity(entries.len());
            for entry in entries {
                let at = parse_instant("history.at", &entry.at)?;
                let binding = match entry.binding {
                    Some(bound) => bound.candidates()?,
                    None => [None].into(),
                };
                timeline.push((at, binding));
            }
            let timeline = Schedule::new([None].into(), timeline)
                .map_err(|e| format!("history.bindings[{todo:?}]: {e}"))?;
            bindings.insert(todo, timeline);
        }
        Ok(History {
            bindings,
            tended: parse_tended("history.tended", raw.tended)?,
        })
    }

    /// The environment at `t`: per todo, the reading in force at `t`, and the
    /// tendings dated at or before it.
    pub fn at(&self, t: Instant) -> fpl::Env {
        let mut env = fpl::Env::new();
        for (todo, timeline) in &self.bindings {
            let candidates = timeline.at(t);
            if *candidates != Candidates::from([None]) {
                env.outcomes.insert(todo.clone(), candidates.clone());
            }
        }
        for (todo, tendings) in &self.tended {
            let known: BTreeSet<Instant> = tendings.range(..=t).copied().collect();
            if !known.is_empty() {
                env.tended.insert(todo.clone(), known);
            }
        }
        env
    }
}

// --- out ---------------------------------------------------------------------

/// One binding, in the shape §7's environment has: the two outcomes are an
/// ADT and never a boolean, because they mean OPPOSITE things downstream.
///
/// ONE ENV SHAPE, and this is it — `{kind, at}` with an ISO instant, which is
/// what `parse_env` reads, and therefore what a caller can hand straight back
/// to `fulfillment` without rewriting anything. [`json_entry`]'s `state`/`at` is a RENDERING of this
/// (lowercased, `isoformat(" ")`), which `Entry::state`/`Entry::at` perform in
/// the core so that both bindings spell it one way.
pub fn json_binding(binding: Outcome) -> Value {
    json!({ "kind": binding.kind(), "at": fpl::iso(binding.at()) })
}

/// §6.1's environment in [`parse_env`]'s shape: the outcomes, and the
/// tendings.
pub fn json_env(env: &Env) -> Value {
    json!({
        "outcomes": Value::Object(
            env.outcomes
                .iter()
                .map(|(todo, candidates)| (todo.clone(), json_outcome(candidates)))
                .collect(),
        ),
        "tended": json_tended(&env.tended),
    })
}

/// One binding as [`json_binding`], a conflict as the array of its
/// candidates with `null` for an open one.
fn json_outcome(candidates: &Candidates) -> Value {
    match candidates.iter().collect::<Vec<_>>()[..] {
        [Some(binding)] => json_binding(*binding),
        ref many => Value::Array(
            many.iter()
                .map(|binding| binding.map_or(Value::Null, json_binding))
                .collect(),
        ),
    }
}

/// An outcome in [`History`]'s shape: `null` for open, else as [`json_env`]
/// writes it.
pub fn json_reading(candidates: &Candidates) -> Value {
    if *candidates == Candidates::from([None]) {
        Value::Null
    } else {
        json_outcome(candidates)
    }
}

/// Per todo, its tendings as ISO instants, ascending.
pub fn json_tended(tended: &BTreeMap<String, BTreeSet<Instant>>) -> Value {
    Value::Object(
        tended
            .iter()
            .map(|(todo, tendings)| {
                (
                    todo.clone(),
                    strings(tendings.iter().map(|at| fpl::iso(*at))),
                )
            })
            .collect(),
    )
}

/// Per todo, its one candidate as `one` renders it, or the array of them
/// under a conflict.
pub fn json_candidates<T>(map: &BTreeMap<TodoId, Vec<T>>, one: impl Fn(&T) -> Value) -> Value {
    Value::Object(
        map.iter()
            .map(|(todo, candidates)| {
                let value = match &candidates[..] {
                    [only] => one(only),
                    many => Value::Array(many.iter().map(&one).collect()),
                };
                (todo.as_str().to_owned(), value)
            })
            .collect(),
    )
}

pub fn json_terms(terms: &BTreeMap<TodoId, Term>) -> Value {
    Value::Object(
        terms
            .iter()
            .map(|(todo, term)| (todo.as_str().to_owned(), crate::json::to_json(term)))
            .collect(),
    )
}

/// A §2 literal as JSON, by the obvious mapping — the ONE reading of a
/// payload's fields this boundary has.
///
/// `None` is `null`, a bool is a bool, a string is a string; an integer is a
/// number where one fits and its print where it does not (§2's integers are
/// unbounded and JSON's are not); a float is a number; a datetime is
/// [`fpl::iso`], the one instant shape this whole file speaks; a timedelta is
/// its three components, which is what it IS; a tuple is an array; and a
/// constructor call is an object of its fields with its name under `"kind"`.
///
/// A nested call whose own field is named `kind` keeps the FIELD and loses the
/// tag, because a reader is after the value; the core kinds and the reference
/// payload have no such field, and a host that adds one should expect it.
///
/// A TERM ARRIVES AS ITS LITERAL SHAPE HERE, not as [`crate::json::to_json`]'s — this
/// mapping is a payload's, and a payload's fields are opaque to this crate. A
/// caller that wants a term as `to_json` reads `fold`'s `specs`/`flatten`, or
/// hands the print to `term_json`.
pub fn json_literal(value: &literal::Value) -> Value {
    match value {
        literal::Value::None => Value::Null,
        literal::Value::Bool(flag) => Value::Bool(*flag),
        literal::Value::Int(int) => int
            .as_i64()
            .map_or_else(|| Value::String(literal::print_literal(value)), Value::from),
        literal::Value::Float(float) => Value::from(float.get()),
        literal::Value::Str(text) => Value::String(text.clone()),
        literal::Value::Datetime(at) => Value::String(fpl::iso(fpl::instant_of(*at))),
        literal::Value::Timedelta(delta) => json!({
            "days": delta.days(),
            "seconds": delta.seconds(),
            "microseconds": delta.microseconds(),
        }),
        literal::Value::Tuple(items) => Value::Array(items.iter().map(json_literal).collect()),
        literal::Value::Call(call) => {
            let mut out = Map::new();
            out.insert("kind".to_owned(), Value::String(call.name.clone()));
            for (name, field) in &call.fields {
                out.insert(name.clone(), json_literal(field));
            }
            Value::Object(out)
        }
    }
}

/// One content record: the three fields §4 gives every event, then the
/// PAYLOAD's own, keyed by the names it stores them under.
///
/// No rendering of a body, and there could not be one: markup is a compiler
/// this core does not carry (nor should — a browser that rendered its own
/// markup would be a second answer to "may this string become nodes"), and a
/// field's meaning is the host's anyway. A caller that renders adds its own
/// keys beside these.
pub fn json_record<P: Payload>(record: &Authored<P>) -> Value {
    let mut out = Map::new();
    out.insert(
        "todo".to_owned(),
        Value::String(record.todo.as_str().to_owned()),
    );
    out.insert(
        "at".to_owned(),
        Value::String(fpl::iso(fpl::instant_of(record.at))),
    );
    out.insert(
        "actor".to_owned(),
        Value::String(record.actor.as_str().to_owned()),
    );
    for (name, value) in record.payload.fields() {
        out.insert(name.to_owned(), json_literal(&value));
    }
    Value::Object(out)
}

/// One §6.7 entry, in the shape the reference's row carried and the
/// wire still carries — one row, one reading, on every host.
///
/// `value` is a number, `"absent"` where the function reads `∅`, or `null`
/// where it does not link, and `unlinked` then says why.
///
/// `spec` is the todo's §6.4 function as its canonical PRINT and not as
/// `to_json`, which is the exception to this file's "terms travel as JSON"
/// rule and is deliberate: it is the value's IDENTITY (§3), it is what the
/// vector compares, and no page reads it. A caller that wants the JSON shape
/// has `term_json`, the §2 → §7 door.
pub fn json_entry<P: Payload>(entry: &Entry<TodoEvent<P>>) -> Value {
    json!({
        "todo": entry.key.as_str(),
        "state": entry.state(),
        "at": entry.at(),
        "claimed": entry.claimed(),
        "value": match entry.value() {
            Ok(Some(value)) => json!(value),
            Ok(None) => json!("absent"),
            Err(_) => Value::Null,
        },
        "unlinked": entry.unlinked().map(ToString::to_string),
        "unconfirmed": entry.confidence.is_provisional(),
        "conflicts": Value::Object(
            entry
                .conflicts
                .iter()
                .map(|(kind, writes)| {
                    (
                        kind.as_str().to_owned(),
                        strings(writes.iter().map(|write| write.as_str().to_owned())),
                    )
                })
                .collect(),
        ),
        "content": match entry.content() {
            [] => Value::Null,
            [one] => json!(one.as_str()),
            many => strings(many.iter().map(|name| name.as_str().to_owned())),
        },
        "spec": fpl::print_term(entry.spec()),
        "stream": strings(entry.stream.iter().map(|name| name.as_str().to_owned())),
        "begun": fpl::iso(fpl::instant_of(entry.begun)),
    })
}

/// One event's identity on a todo's timeline — `json_series`'s marker,
/// and what a locally folded entry's `stream` is a list of.
pub fn json_marker<P: Payload>(name: &Hash, event: &TodoEvent<P>) -> Value {
    json!({
        "at": fpl::iso(fpl::instant_of(event.at())),
        "kind": event.kind_name(),
        "actor": event.actor().as_str(),
        "hash": name.as_str(),
    })
}

pub fn object(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(
        pairs
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<Map<String, Value>>(),
    )
}

pub fn strings(items: impl IntoIterator<Item = String>) -> Value {
    Value::Array(items.into_iter().map(Value::String).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prodrome::event::Hash;
    use prodrome::fold::Kind;
    use prodrome::fpl::{instant_of, mk_flat};
    use prodrome::literal::Datetime;
    use prodrome::reference::Todo;
    use prodrome::todo::Reading;
    use prodrome::view::{Confidence, Price, Provisional};

    type Row = Entry<TodoEvent<Todo>>;

    fn at(day: u32) -> Datetime {
        Datetime::new(2026, 9, day, 12, 0, 0, 0).expect("a real instant")
    }

    fn name(byte: char) -> Hash {
        Hash::new(byte.to_string().repeat(64)).expect("64 hex")
    }

    /// THE ENTRY'S WIRE SHAPE (§6.7), pinned here rather than inferred from
    /// the struct: `conformance/view/*.py` freezes ten of these fields' VALUES
    /// in the core, and this freezes the JSON keys and forms the browser reads
    /// them under — the lowercased state, `isoformat(" ")`, `""` for an absent
    /// claim, `"absent"` for an absent value, a term as its §2 PRINT and
    /// never as `to_json`'s shape, and `begun` as an ISO instant, which the
    /// page compares with the answer's `at` (law 87).
    #[test]
    fn an_entry_crosses_as_the_twelve_keys_the_page_reads() {
        let entry: Row = Entry {
            genesis: None,
            key: TodoId::new("alpha").expect("valid"),
            reading: Reading {
                outcome: [Some(Outcome::Completed(instant_of(at(3))))].into(),
                content: vec![name('a')],
            },
            claim: None,
            confidence: Confidence::Provisional(Provisional::Content),
            price: Price {
                spec: mk_flat(0.25).expect("valid"),
                value: Ok(Some(0.25)),
                linked: Ok(fpl::Closed::of(mk_flat(0.25).expect("valid")).expect("closed")),
            },
            conflicts: [(Kind::State, vec![name('b'), name('c')])]
                .into_iter()
                .collect(),
            stream: vec![name('a'), name('b')],
            begun: at(1),
        };
        assert_eq!(
            json_entry(&entry),
            json!({
                "todo": "alpha",
                "state": "completed",
                "at": "2026-09-03 12:00:00",
                "claimed": "",
                "value": 0.25,
                "unlinked": Value::Null,
                "unconfirmed": true,
                "conflicts": {"state": ["b".repeat(64), "c".repeat(64)]},
                "content": "a".repeat(64),
                "spec": "Flat(value=0.25)",
                "stream": ["a".repeat(64), "b".repeat(64)],
                "begun": "2026-09-01T12:00:00",
            })
        );
    }

    /// The absences, which are the half a shape test usually misses: an OPEN
    /// todo with no record and no price is `"open"`, `""`, `"absent"`,
    /// `Absent()` and `null` — never a zero and never a missing key.
    #[test]
    fn an_open_unpriced_todo_crosses_as_absences_and_not_as_zeroes() {
        let entry: Row = Entry {
            genesis: None,
            key: TodoId::new("beta").expect("valid"),
            reading: Reading {
                outcome: [None].into(),
                content: vec![],
            },
            claim: Some(Reading {
                outcome: [Some(Outcome::Cancelled(instant_of(at(5))))].into(),
                content: vec![],
            }),
            confidence: Confidence::Provisional(Provisional::Claimed),
            price: Price {
                spec: fpl::mk_absent(),
                value: Ok(None),
                linked: Ok(fpl::Closed::of(fpl::mk_absent()).expect("closed")),
            },
            conflicts: BTreeMap::new(),
            stream: vec![],
            begun: at(1),
        };
        assert_eq!(
            json_entry(&entry),
            json!({
                "todo": "beta",
                "state": "open",
                "at": "",
                "claimed": "cancelled",
                "value": "absent",
                "unlinked": Value::Null,
                "unconfirmed": true,
                "conflicts": {},
                "content": Value::Null,
                "spec": "Absent()",
                "stream": [],
                "begun": "2026-09-01T12:00:00",
            })
        );
    }

    /// `link`'s two arguments and its answer, as the browser holds them: the
    /// linked term is closed, so it is what `fulfillment` reads next.
    #[test]
    fn a_ref_links_over_json_and_the_answer_evaluates() {
        let term = parse_term(
            "term",
            r#"{"kind": "conj", "p": -1.0, "terms": [{"kind": "ref", "todo": "a"}, {"kind": "ref", "todo": "b"}]}"#,
        )
        .expect("a term");
        let specs = parse_specs(
            r#"{"a": {"kind": "flat", "value": 0.25}, "b": {"kind": "ref", "todo": "a"}}"#,
        )
        .expect("specs");
        let linked = fpl::link(&term, &specs).expect("links");
        let json = crate::json::to_json(linked.term());
        assert_eq!(
            json,
            json!({"kind": "conj", "p": -1.0, "terms": [
                {"kind": "flat", "value": 0.25}, {"kind": "flat", "value": 0.25}
            ]})
        );
        let closed = parse_closed("term", &json.to_string()).expect("closed");
        let now = instant_of(at(1));
        assert_eq!(fpl::fulfillment(&closed, now, &fpl::Env::new()), Some(0.25));
    }

    /// The environment crosses as its two halves and comes back as the core's:
    /// what `fold` emits is what `fulfillment` reads, and a `recur` term reads
    /// the tendings in it. `{}` is the empty environment; an environment in
    /// the pre-0.8 shape, a bare map of outcomes, is refused and not misread.
    #[test]
    fn the_environment_crosses_with_its_tendings() {
        let mut env = Env::new();
        env.bind("brush", Outcome::Completed(instant_of(at(2))));
        env.tended.insert(
            "brush".to_owned(),
            [instant_of(at(1)), instant_of(at(10))].into(),
        );
        let json = json_env(&env);
        assert_eq!(
            json,
            json!({
                "outcomes": {"brush": {"kind": "Completed", "at": "2026-09-02T12:00:00"}},
                "tended": {"brush": ["2026-09-01T12:00:00", "2026-09-10T12:00:00"]},
            })
        );
        let back = parse_env(&json.to_string()).expect("the shape fold emits");
        assert_eq!(back, env);

        let recur = parse_closed(
            "term",
            r#"{"kind": "recur", "todo": "brush", "anchor": "2026-09-01T12:00:00",
                "term": {"kind": "curve", "points": [
                    {"at": "2026-09-01T12:00:00", "value": 1.0},
                    {"at": "2026-09-05T12:00:00", "value": 0.0}]},
                "pending": {"kind": "flat", "value": 0.3}}"#,
        )
        .expect("a closed term");
        assert_eq!(
            fpl::fulfillment(&recur, instant_of(at(3)), &back),
            Some(0.5)
        );
        assert_eq!(
            fpl::fulfillment(&recur, instant_of(at(10)), &back),
            Some(1.0)
        );
        let empty = parse_env("{}").expect("the empty environment");
        assert_eq!(
            fpl::fulfillment(&recur, instant_of(at(3)), &empty),
            Some(0.3)
        );
        assert!(
            parse_env(r#"{"brush": {"kind": "Completed", "at": "2026-09-02T12:00:00"}}"#).is_err()
        );
    }

    /// The history crosses the same way: a tending is in force at a knot from
    /// its own instant on, never before.
    #[test]
    fn the_history_crosses_with_its_tendings() {
        let past = History::parse(
            r#"{"bindings": {"a": [{"at": "2026-09-02T12:00:00",
                    "binding": {"kind": "Cancelled", "at": "2026-09-02T12:00:00"}}]},
                "tended": {"brush": ["2026-09-05T12:00:00"]}}"#,
        )
        .expect("a history");
        assert!(past.at(instant_of(at(4))).tended.is_empty());
        assert_eq!(past.at(instant_of(at(4))).outcomes.len(), 1);
        assert_eq!(past.at(instant_of(at(5))).tended["brush"].len(), 1);
        assert!(History::parse(r#"{"a": []}"#).is_err());
    }

    /// A conflict in the history crosses as its candidates, and `null` is
    /// open again from its instant on.
    #[test]
    fn the_history_crosses_a_conflict() {
        let past = History::parse(
            r#"{"bindings": {"a": [
                {"at": "2026-09-02T12:00:00",
                 "binding": [null, {"kind": "Completed", "at": "2026-09-02T12:00:00"}]},
                {"at": "2026-09-06T12:00:00", "binding": null}]}}"#,
        )
        .expect("a history");
        assert!(past.at(instant_of(at(1))).outcomes.is_empty());
        assert_eq!(past.at(instant_of(at(4))).outcomes["a"].len(), 2);
        assert!(past.at(instant_of(at(7))).outcomes.is_empty());
    }

    /// A conflict crosses as its candidates: the state joined by `|`, the
    /// records' names and the environment's outcome as arrays, `null` open.
    #[test]
    fn a_conflict_crosses_as_its_candidates() {
        let candidates: Candidates = [None, Some(Outcome::Completed(instant_of(at(3))))].into();
        let mut env = Env::new();
        env.outcomes.insert("alpha".to_owned(), candidates.clone());
        let json = json_env(&env);
        assert_eq!(
            json["outcomes"]["alpha"],
            json!([Value::Null, {"kind": "Completed", "at": "2026-09-03T12:00:00"}])
        );
        assert_eq!(parse_env(&json.to_string()).expect("round-trips"), env);

        let entry: Row = Entry {
            genesis: None,
            key: TodoId::new("alpha").expect("valid"),
            reading: Reading {
                outcome: candidates,
                content: vec![name('a'), name('b')],
            },
            claim: None,
            confidence: Confidence::Confirmed,
            price: Price {
                spec: fpl::mk_absent(),
                value: Ok(None),
                linked: Ok(fpl::Closed::of(fpl::mk_absent()).expect("closed")),
            },
            conflicts: BTreeMap::new(),
            stream: vec![],
            begun: at(1),
        };
        let json = json_entry(&entry);
        assert_eq!(json["state"], "open|completed");
        assert_eq!(json["at"], "2026-09-03 12:00:00");
        assert_eq!(json["content"], json!(["a".repeat(64), "b".repeat(64)]));
    }

    #[test]
    fn an_open_term_is_refused_where_a_closed_one_is_read() {
        let refusal = parse_closed("term", r#"{"kind": "ref", "todo": "a"}"#);
        assert_eq!(
            refusal.expect_err("an open term"),
            "term: holds a ref; link it first"
        );
        assert!(parse_specs(r#"{"a": {"kind": "ref", "todo": "Not An Id"}}"#).is_err());
    }

    /// A function that does not link crosses with no value and the reason.
    #[test]
    fn an_unlinked_entry_crosses_with_its_reason() {
        let entry: Row = Entry {
            genesis: None,
            key: TodoId::new("alpha").expect("valid"),
            reading: Reading {
                outcome: [None].into(),
                content: vec![],
            },
            claim: None,
            confidence: Confidence::Confirmed,
            price: Price {
                spec: fpl::mk_ref("ghost".to_owned()).expect("valid"),
                value: Err(fpl::LinkError::Unknown("ghost".to_owned())),
                linked: Err(fpl::LinkError::Unknown("ghost".to_owned())),
            },
            conflicts: BTreeMap::new(),
            stream: vec![],
            begun: at(1),
        };
        let json = json_entry(&entry);
        assert_eq!(json["value"], Value::Null);
        assert_eq!(json["unlinked"], "Ref(\"ghost\") names no known todo");
        assert_eq!(json["spec"], "Ref(todo='ghost')");
    }
}
