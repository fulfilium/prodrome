//! The REFERENCE module's exports: the todo schema at the reference payload,
//! every one answering as it did before the exports were generic over a
//! schema (`snapshots/dag-1.txt`). See the crate's header for the surface.

use std::collections::{BTreeMap, BTreeSet};

use prodrome::event::{Envelope, Hash, TodoEvent};
use prodrome::fold::{self, Product};
use prodrome::fpl::{datetime_of, instant_of, iso, print_term, scalars};
use prodrome::policy::Policy;
use prodrome::registers;
use prodrome_wasm_exports::{self as exports, json, name_of, Read, Replica, Sender};
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

use prodrome_wasm_exports::wire::{
    json_candidates, json_entry, json_env, json_marker, json_next_change, json_reading,
    json_record, json_tended, json_terms, object, parse_closed, parse_env, parse_instant,
    parse_moment, parse_names, parse_observation, parse_specs, parse_stores, parse_term,
    parse_untrusted, strings, History, Refusal,
};

/// THE RECORD SHAPE THIS MODULE WAS BUILT WITH: `prodrome::reference::Todo`,
/// the shape `conformance/*.py` was taken with. A host with its own payload,
/// or its own schemas, builds its own module with
/// `prodrome_wasm_exports::schema!` (see that crate's header).
type Record = prodrome::reference::Todo;

type Event = TodoEvent<Record>;
type Object = Envelope<Event>;

// The todo schema's exports through the generic path, beside the free
// functions below that have always answered for it: `new Todos(objects)`.
prodrome_wasm_exports::schema!(Todos = Event, priced);

// Any schema declared as data, given at construction and admitted when its
// laws hold: `new Declared(schemaText, objects)`.
prodrome_wasm_exports::schema!(Declared, declared);

/// A refusal, as the exception a JS caller catches. Every entry point returns
/// one rather than panicking: a browser that aborts inside the Wasm leaves the
/// module poisoned for the rest of the session, and the SubtleCrypto fallback
/// can only run if the failure arrived as a value.
fn refused(reason: Refusal) -> JsError {
    JsError::new(&reason)
}

fn printed(value: &Value) -> Result<String, JsError> {
    exports::printed(value).map_err(refused)
}

/// The crate version, so a page can say WHICH core answered it.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// SPEC §2's printable set: its Unicode version and its SHA-256
/// ([`exports::printable`]).
#[wasm_bindgen]
pub fn printable() -> String {
    exports::printable()
}

// --- §3: verify --------------------------------------------------------------

/// §3's `verify`, for the set of objects a browser was handed: the reference
/// schema's [`exports::verify`].
///
/// `objects` is `[{hash, text}]` — every object's claimed name beside the exact
/// canonical print that name is a hash of.
#[wasm_bindgen]
pub fn verify_objects(objects: &str) -> Result<String, JsError> {
    exports::verify(&Replica::<Event>::of(objects, Sender::Stranger).map_err(refused)?)
        .map_err(refused)
}

/// Per todo, its events in causal order with the object that carries each —
/// `json_entry`'s `stream` and `json_series`'s `markers`, from one pass.
fn streams(read: &Read<'_, Event>) -> BTreeMap<String, Vec<(Hash, Event)>> {
    let mut out: BTreeMap<String, Vec<(Hash, Event)>> = BTreeMap::new();
    for node in read.nodes {
        if let Some(event) = node.event() {
            out.entry(event.todo().as_str().to_owned())
                .or_default()
                .push((node.name().clone(), event.clone()));
        }
    }
    out
}

// --- §6: the folds -----------------------------------------------------------

/// §6.1–6.5 at `at` (ISO, or `null` for everything the chain holds), under
/// `untrusted` — a list of actor names, read as the REFERENCE policy (§5),
/// which arrives from the server because WHICH actors a host stands behind is
/// instance knowledge and a core that guessed it would fold a different chain
/// while claiming to fold the same one. The wire is what it always was; since
/// 0.3 the core takes a `policy::Policy` and this array becomes one.
///
/// The answer is the five folds, each keyed by todo id and shaped the way
/// `view.py` already puts it on the wire:
///
/// - `env`      — §6.1, as `{outcomes, tended}` — §7's environment shape, so
///                it is also a legal argument to [`fulfillment`] and [`explain`]
/// - `specs`    — §6.2, each a [`json::to_json`] term
/// - `content`  — §6.3, each the reference's `json_authored` MINUS `rich`
/// - `flatten`  — §6.4, each a [`json::to_json`] term: the todo's fulfillment
///                FUNCTION, which is what a `value` and a series are read off
/// - `functions`— every todo the objects mention, each its `flatten` function
///                or `absent` where it has none: what a `ref` links against
///                (§7.2), and so [`link`]'s `specs` for a chain
/// - `history`  — §6.5, as `{bindings, tended}`, and the argument
///                [`series_knots`] wants, so that a knot in the past is
///                evaluated against the environment as of that past instant
///
/// `stream` rides beside them — per todo, its events in causal order with the
/// object carrying each — because an `Entry` on the wire has one, and a locally
/// folded list that dropped it would be a DIFFERENT list rather than the same
/// one computed here.
#[wasm_bindgen]
pub fn fold(objects: &str, at: Option<String>, untrusted: &str) -> Result<String, JsError> {
    let replica = Replica::<Event>::of(objects, Sender::Stranger).map_err(refused)?;
    let read = replica.read().and_then(Read::whole).map_err(refused)?;
    let policy = parse_untrusted(untrusted).map_err(refused)?;
    let moment = read
        .moment(parse_moment("at", at).map_err(refused)?)
        .map_err(refused)?;
    let state = registers::fold(read.nodes);
    let now = instant_of(moment);
    let mut env = prodrome::fpl::Env::new();
    let (mut specs, mut content, mut flat) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    for prodrome in state.prodromes().values() {
        let one = fold::env(prodrome, now, &policy);
        env.outcomes.extend(one.outcomes);
        env.tended.extend(one.tended);
        specs.extend(fold::specs(prodrome, now, &policy));
        content.extend(fold::content(prodrome, now));
        flat.extend(fold::flatten(prodrome, now, &policy).map_err(|e| refused(e.0))?);
    }
    let functions = Value::Object(
        fold::link_specs(&flat, state.entities().map(|(todo, _)| todo))
            .into_iter()
            .map(|(todo, term)| (todo, json::to_json(&term)))
            .collect(),
    );

    // Each todo's candidate bindings as a step function of time: its
    // readings mapped to their outcomes, a knot where they change; and every
    // binding tending. What `series_knots` reads back.
    let mut bindings = serde_json::Map::new();
    let mut tended = BTreeMap::new();
    for (todo, stream) in state.entities() {
        for stamp in stream
            .iter()
            .filter(|s| policy.standing(&s.name, &*s.event).binds())
        {
            for write in fold::Write::of(&stamp.event) {
                if let fold::Write::Tend(at) = write {
                    tended
                        .entry(todo.as_str().to_owned())
                        .or_insert_with(BTreeSet::new)
                        .insert(at);
                }
            }
        }
        let outcomes = fold::readings(stream, None, &policy)
            .map(|registers| registers.outcomes())
            .normal();
        let timeline: Vec<Value> = outcomes
            .knots()
            .iter()
            .map(|(at, reading)| json!({ "at": iso(*at), "binding": json_reading(reading) }))
            .collect();
        if !timeline.is_empty() {
            bindings.insert(todo.as_str().to_owned(), Value::Array(timeline));
        }
    }
    let history = json!({ "bindings": bindings, "tended": json_tended(&tended) });
    let stream = Value::Object(
        streams(&read)
            .iter()
            .map(|(todo, events)| {
                (
                    todo.clone(),
                    Value::Array(
                        events
                            .iter()
                            .map(|(name, event)| json_marker(name, event))
                            .collect(),
                    ),
                )
            })
            .collect(),
    );

    printed(&object(vec![
        ("at", Value::String(iso(instant_of(moment)))),
        ("env", json_env(&env)),
        ("specs", json_candidates(&specs, json::to_json)),
        ("content", json_candidates(&content, json_record)),
        ("flatten", json_terms(&flat)),
        ("functions", functions),
        ("history", history),
        ("stream", stream),
    ]))
}

/// §6.6 — the registers whose frontier holds more than one write: a DAG's
/// concurrent writes that no later write has settled, by todo and by register
/// kind, each named by the object that made it.
///
/// `{}` on a chain, which is why this is a separate call and not a sixth key
/// on [`fold`]: the answer is empty on today's store, and an ancestry bitset
/// per object is not a cost to pay on every fold to learn that again.
#[wasm_bindgen]
pub fn registers(objects: &str, at: Option<String>, untrusted: &str) -> Result<String, JsError> {
    let replica = Replica::<Event>::of(objects, Sender::Stranger).map_err(refused)?;
    let read = replica.read().and_then(Read::whole).map_err(refused)?;
    let policy = parse_untrusted(untrusted).map_err(refused)?;
    let moment = parse_moment("at", at)
        .map_err(refused)?
        .map(datetime_of)
        .transpose()
        .map_err(|e| refused(format!("at: {e}")))?;
    let state = registers::fold(read.nodes);
    let conflicts = Value::Object(
        state
            .entities()
            .filter_map(|(todo, stream)| {
                let found = fold::read(stream, moment.map(instant_of), &policy).conflicts();
                (!found.is_empty()).then(|| {
                    let by_kind = found
                        .into_iter()
                        .map(|(kind, names)| {
                            let names = names.iter().map(|name| name.as_str().to_owned());
                            (kind.as_str().to_owned(), strings(names))
                        })
                        .collect();
                    (todo.as_str().to_owned(), Value::Object(by_kind))
                })
            })
            .collect(),
    );
    printed(&object(vec![("conflicts", conflicts)]))
}

/// §6.7 — every todo the chain has ever mentioned, as the folds see it at `at`
/// (ISO, or `null` for everything the chain holds), under `untrusted`.
///
/// THE COMPOSITION IS THE CORE'S. Until this existed the tab performed it
/// itself — two [`fold`]s, a [`registers`] call, and the page's TypeScript deciding
/// what a claim is, when an entry is provisional, and which environment prices
/// it. That was a second reading of §6.7 in a second language,
/// which is exactly what this crate exists to prevent, and it is gone.
///
/// `records` is the lookup an entry's `content` NAME implies: an entry carries
/// the name of the winning content object and never the record, and a browser
/// holding those objects as TEXT cannot open one without the §2 parser — which
/// is here. So the answer carries the records the rows actually name, and
/// nothing else: it is the caller's own lookup done on the caller's behalf,
/// not the Prodrome deciding what a body is for. The shape is
/// [`wire::json_record`] — `todo`, `at`, `actor` and the payload's own fields
/// under the names it stores them by.
///
/// `order` is the rows' todo ids in §6.7's list order ([`prodrome::view::list_order`]):
/// most urgent first, then every row with no number — `absent`, or not
/// linking — by id. A page lists in it and does not sort for itself.
///
/// `created` is the other half of that lookup, for a todo with NO record: per
/// todo, the instant of its first `Created` and the text of its last — what
/// `prodrome list` shows as the body when there is no record to read one
/// from. Every `Created` in the chain counts, before `at` or after, exactly
/// as the command line gathers them; the rows say what `at` believes. Whether
/// a todo exists at `at` is each row's `begun`, and not `created`: an item
/// that content records alone make has no `Created` (law 87).
#[wasm_bindgen]
pub fn entries(objects: &str, at: Option<String>, untrusted: &str) -> Result<String, JsError> {
    let replica = Replica::<Event>::of(objects, Sender::Stranger).map_err(refused)?;
    let read = replica.read().and_then(Read::whole).map_err(refused)?;
    let policy = parse_untrusted(untrusted).map_err(refused)?;
    let moment = read
        .moment(parse_moment("at", at).map_err(refused)?)
        .map_err(refused)?;
    let rows =
        prodrome::view::entries(read.nodes, moment, &policy).map_err(|e| refused(e.to_string()))?;
    let mut listed: Vec<&prodrome::view::Entry<Event>> = rows.iter().collect();
    listed.sort_by(|a, b| prodrome::view::list_order(a, b));
    let records: serde_json::Map<String, Value> = rows
        .iter()
        .flat_map(prodrome::view::Entry::content)
        .filter_map(|name| match read.dag.get(name).and_then(Envelope::event) {
            Some(TodoEvent::Authored(record)) => {
                Some((name.as_str().to_owned(), json_record(record)))
            }
            _ => None,
        })
        .collect();
    let mut created: BTreeMap<String, (String, String)> = BTreeMap::new();
    for event in read.events() {
        if let TodoEvent::Created(event) = event {
            let at = iso(instant_of(event.at));
            created
                .entry(event.todo.as_str().to_owned())
                .and_modify(|(_, text)| text.clone_from(&event.text))
                .or_insert((at, event.text.clone()));
        }
    }
    let created: serde_json::Map<String, Value> = created
        .into_iter()
        .map(|(todo, (at, text))| (todo, json!({ "at": at, "text": text })))
        .collect();
    printed(&object(vec![
        ("at", Value::String(iso(instant_of(moment)))),
        (
            "entries",
            Value::Array(rows.iter().map(json_entry).collect()),
        ),
        (
            "order",
            strings(listed.iter().map(|row| row.key.as_str().to_owned())),
        ),
        ("records", Value::Object(records)),
        ("created", Value::Object(created)),
    ]))
}

// --- §3 and §4: SEALING, so a replica can append ------------------------------

/// A lifecycle event, as its CANONICAL PRINT — §4's five same-shaped kinds
/// (`Created`, `Completed`, `Cancelled`, `Reopened`, `Tended`), through the very `mk_*`
/// constructors that are their parse boundary.
///
/// This exists because a replica has to be able to append WITH NO NETWORK, and
/// the only alternative was a printer written in TypeScript — a second
/// implementation of §2's grammar, which is exactly what this crate exists to
/// prevent. The caller supplies four strings; the core decides whether they are
/// a value, spells it, and refuses if they are not.
///
/// The answer is a print rather than JSON because the print IS the value's
/// identity (§3): it is what [`seal`] hashes, what the file holds, and what
/// crosses every other boundary in this system.
///
/// ⚠️ `at` IS THE CALLER'S CLAIM AND NOTHING CLAMPS IT (§1). A device with a
/// wrong clock produces an event dated in the future; `verify` reports what it
/// must and no layer here rewrites it.
#[wasm_bindgen]
pub fn lifecycle(
    kind: &str,
    todo: &str,
    at: &str,
    actor: &str,
    note: &str,
) -> Result<String, JsError> {
    let at = parse_instant("at", at).map_err(refused)?;
    let at = datetime_of(at).map_err(|e| refused(format!("at: {e}")))?;
    let event: Event = match kind {
        "Created" => prodrome::event::mk_created(todo, at, actor, "", note),
        "Completed" => prodrome::event::mk_completed(todo, at, actor, note),
        "Cancelled" => prodrome::event::mk_cancelled(todo, at, actor, note),
        "Reopened" => prodrome::event::mk_reopened(todo, at, actor, note),
        "Tended" => prodrome::event::mk_tended(todo, at, actor, note),
        other => {
            return Err(refused(format!(
                "kind must be one of Created, Completed, Cancelled, Reopened, Tended, got {other:?}"
            )));
        }
    }
    .map_err(|e| refused(e.to_string()))?;
    Ok(prodrome::event::canonical(&event))
}

/// One object's name and its bytes, the way `/api/objects` carries them.
fn object_of(envelope: &Object) -> Result<String, JsError> {
    let literal = prodrome::event::canonical_envelope(envelope);
    printed(&object(vec![
        ("name", Value::String(name_of(&literal))),
        ("literal", Value::String(literal)),
    ]))
}

/// The event a sealing call was given, or `None` — a print, read back through
/// the closed vocabulary and every `mk_*` rule, so a caller cannot seal a
/// record the constructors would have refused.
fn event_of(event: Option<String>) -> Result<Option<Event>, JsError> {
    event
        .map(|print| {
            prodrome::event::parse_event(&Default::default(), &print)
                .map_err(|e| refused(format!("event: {e}")))
        })
        .transpose()
}

/// §3 — SEAL AN EVENT ONTO THE HEADS A REPLICA HOLDS, and answer with the name
/// and the bytes.
///
/// `prev` is a JSON array of head names: none is genesis (`Sealed(prev='')`),
/// one is an ordinary `Sealed`, and two or more is a `Woven` that writes and
/// joins at once. `event` is a lifecycle print from [`lifecycle`] or an
/// `Authored` print the box authored (`POST /api/author`), and `null` is only
/// legal with two parents or more, where it is a plain merge — see
/// [`merge_object`], which is the door that says so in its name.
///
/// THIS IS THE WHOLE OF WHAT A PHONE NEEDS TO WRITE. Print through the core's
/// printer, hash the print: the name is `sha256(utf8(print))`, which is what
/// the store's filename is and what the box rehashes on receipt. So a replica
/// and a box independently agree on what an object is CALLED, with nothing in
/// between them but bytes.
#[wasm_bindgen]
pub fn seal(prev: &str, event: Option<String>) -> Result<String, JsError> {
    let parents = parse_names("prev", prev).map_err(refused)?;
    let event = event_of(event)?;
    let envelope = match (parents.len(), event) {
        (0, Some(event)) => prodrome::event::mk_sealed(None, event),
        (1, Some(event)) => prodrome::event::mk_sealed(parents.into_iter().next(), event),
        (_, event) => {
            prodrome::event::mk_woven(parents, event).map_err(|e| refused(e.to_string()))?
        }
    };
    object_of(&envelope)
}

/// §3 — A MERGE: `Woven(parents, event)` over two heads or more.
///
/// `event` is `null` for the ordinary case, and that is the design rather than
/// an omission: a merge asserts STRUCTURE, not a fact about a todo, so it
/// carries no author, no instant and no id. Two replicas joining the same
/// heads therefore produce the same bytes — `mk_woven` sorts the parents — and
/// the union of two histories stays a union however many times it is taken.
///
/// A caller that wants the join ATTRIBUTED passes an ordinary event print, and
/// the object is a write and a join at once.
#[wasm_bindgen]
pub fn merge_object(parents: &str, event: Option<String>) -> Result<String, JsError> {
    let parents = parse_names("parents", parents).map_err(refused)?;
    let event = event_of(event)?;
    let envelope = prodrome::event::mk_woven(parents, event).map_err(|e| refused(e.to_string()))?;
    object_of(&envelope)
}

// --- §7: the evaluator -------------------------------------------------------

/// §7 — what a term is worth at an instant, under an environment.
///
/// A `f64` and not a printed number: this is the one answer a caller does
/// arithmetic on (the graph page compares it with the server's knot to 1e-9),
/// and a decimal string in between would be a rounding nobody asked for.
/// `undefined` is `∅`: the term has no value there, which is not a number.
#[wasm_bindgen]
pub fn fulfillment(term: &str, now: &str, env: &str) -> Result<Option<f64>, JsError> {
    let term = parse_closed("term", term).map_err(refused)?;
    let now = parse_instant("now", now).map_err(refused)?;
    let env = parse_env(env).map_err(refused)?;
    Ok(prodrome::fpl::fulfillment(&term, now, &env))
}

/// §7 — when `term`'s value, read through `observation` (`{levels,
/// rounding}`, a whole percent being `{"levels": 100, "rounding":
/// "nearest"}`), next changes after `now` under `env`: `{at, exact}`, `at`
/// an ISO instant or `null` for never, and `exact` where it is the observed
/// value's next step rather than a bound never later than it. A page
/// schedules its next redraw at `at` instead of on a ticking clock.
///
/// # Errors
///
/// A term that is not closed, an instant, environment or observation that
/// does not parse: the thrown refusal.
#[wasm_bindgen]
pub fn next_change(term: &str, now: &str, env: &str, observation: &str) -> Result<String, JsError> {
    let term = parse_closed("term", term).map_err(refused)?;
    let now = parse_instant("now", now).map_err(refused)?;
    let env = parse_env(env).map_err(refused)?;
    let observation = parse_observation(observation).map_err(refused)?;
    printed(&json_next_change(prodrome::observe::next_change(
        &term,
        now,
        &env,
        observation,
    )))
}

/// §2 → §7: a term as the CHAIN stores it — its canonical print, the text
/// inside a `SpecRevised` or an `Authored` — read back as the JSON shape
/// everything else here speaks.
///
/// The one place the literal grammar is reachable from JavaScript, and it
/// exists because the grammar is the STORED form: `conformance/series.py`
/// gives each term as a print, an object's literal carries its spec as a
/// print, and without this the two would have to be re-parsed on the JS side —
/// which is the second implementation of §2 that this crate exists to prevent.
#[wasm_bindgen]
pub fn term_json(literal: &str) -> Result<String, JsError> {
    let term = prodrome::fpl::parse_term(literal).map_err(|e| refused(format!("term: {}", e.0)))?;
    printed(&json::to_json(&term))
}

/// §7's explain tree — `Cofree TermF Annotation` as [`json::explain`] writes it: the
/// term's own shape, each node carrying the value it had at the moment its
/// parent used it. A DECORATION, never a second reading.
#[wasm_bindgen]
pub fn explain(term: &str, now: &str, env: &str) -> Result<String, JsError> {
    let term = parse_closed("term", term).map_err(refused)?;
    let now = parse_instant("now", now).map_err(refused)?;
    let env = parse_env(env).map_err(refused)?;
    printed(&json::explain(&term, now, &env))
}

/// §7.1 — a term with every `After` resolved against a snapshot, so a page
/// that redraws a curve stops paying for a lookup per node per sample.
///
/// The compiled term comes back as its canonical PRINT and not as the JSON
/// shape, because a compiled term is a term and [`term_json`] already reads
/// one — and because the print is what a caller would cache. Evaluate it with
/// `fulfillment(compiled, now, "{}")`: the empty environment, which is the
/// whole claim, since nothing is left in it that could read one.
///
/// `links` is not decoration. A cancelled upstream compiles to a 1.0 that no
/// reader could otherwise tell from a demand met (§7), and this is where the
/// boundary is told which is which.
#[wasm_bindgen]
pub fn compile(term: &str, env: &str) -> Result<String, JsError> {
    let term = parse_closed("term", term).map_err(refused)?;
    let env = parse_env(env).map_err(refused)?;
    let compiled = prodrome::chain::compile(&term, &env);
    printed(&object(vec![
        ("term", Value::String(print_term(compiled.term().term()))),
        (
            "links",
            Value::Array(
                compiled
                    .links()
                    .iter()
                    .map(|link| json::scalars_json(&scalars(&link.fields())))
                    .collect(),
            ),
        ),
    ]))
}

/// §7.2 — every `ref` in `term` bound to the todo it names, recursively.
///
/// `specs` is `{"<todo>": <term>}`, each todo's own function in the JSON shape
/// (a `fold`'s `flatten`, for a chain). The answer is the linked term in that
/// same shape, CLOSED, so it goes straight back into [`fulfillment`],
/// [`explain`], [`compile`] and [`series_knots`], which refuse a term that
/// still holds a `ref`. An unknown todo or a loop is the thrown refusal, the
/// loop named.
#[wasm_bindgen]
pub fn link(term: &str, specs: &str) -> Result<String, JsError> {
    link_in(term, specs, "[]")
}

/// §7.2's `link_in`: [`link`], with `stores` the set of prodromes a
/// qualified reference (`refIn`) reads, as the `prices` answer of the
/// stores' objects puts them (`[{genesis, env, functions}]`, see
/// [`parse_stores`]). A `refIn` to a member binds to that entity's function
/// there, linked and compiled in that member, so the term reads what the
/// entity reads in its own prodrome (law 41); one to a store or an entity
/// the set does not hold reads `∅`. `link` is this with no stores.
#[wasm_bindgen]
pub fn link_in(term: &str, specs: &str, stores: &str) -> Result<String, JsError> {
    let term = parse_term("term", term).map_err(refused)?;
    let specs = parse_specs(specs).map_err(refused)?;
    let stores = parse_stores(stores).map_err(refused)?;
    let linked = prodrome::fpl::link_in(&term, &specs, &stores)
        .map_err(|e| refused(format!("link: {e}")))?;
    printed(&json::to_json(linked.term()))
}

/// §7's knots — the curve a graph draws over `[from, to]`, and whether the
/// knots ARE the curve. A knot's value is `null` where the term reads `∅`.
///
/// `history` is the environment as a function of time ([`fold`]'s key of that
/// name), because every knot is evaluated against the environment AS OF its own
/// instant: a dependency an `After` reads is unbound before it completed and
/// bound from then, exactly as `view.series_of` does it.
#[wasm_bindgen]
pub fn series_knots(term: &str, from: &str, to: &str, history: &str) -> Result<String, JsError> {
    let term = parse_closed("term", term).map_err(refused)?;
    let from = parse_instant("from", from).map_err(refused)?;
    let to = parse_instant("to", to).map_err(refused)?;
    let past = History::parse(history).map_err(refused)?;
    let series = prodrome::breaks::series_knots(&term, from, to, |at| past.at(at));
    printed(&object(vec![
        (
            "knots",
            Value::Array(
                series
                    .knots
                    .iter()
                    .map(|knot| {
                        Value::Array(vec![Value::String(iso(knot.at)), Value::from(knot.value)])
                    })
                    .collect(),
            ),
        ),
        ("exact", Value::Bool(series.exact)),
    ]))
}

#[cfg(test)]
mod tests {
    use prodrome::change::mk_change;
    use prodrome::event::{canonical_envelope, mk_created, seal_hash, Envelope};
    use prodrome::genesis::mk_genesis;
    use prodrome::literal::Datetime;
    use serde_json::{json, Value};

    /// A decay read in whole percents steps where its line crosses one, and
    /// an observation is refused, never guessed, where it is malformed.
    #[test]
    fn a_term_answers_its_next_observed_change() {
        let decay = r#"{"kind": "decay", "start": 0.9, "end": 0.1,
            "endDate": "2026-10-01T00:00:00", "leadUpHours": 100.0}"#;
        let percent = r#"{"levels": 100, "rounding": "nearest"}"#;
        let answer = |now: &str, observation: &str| {
            super::next_change(decay, now, "{}", observation)
                .map(|text| serde_json::from_str::<Value>(&text).expect("JSON"))
        };
        let before = answer("2026-09-20T00:00:00", percent).unwrap_or_else(|_| panic!("answers"));
        assert_eq!(
            before,
            json!({"at": "2026-09-26T20:00:00", "exact": true}),
            "0.98 until the window opens"
        );
        let after = answer("2026-10-02T00:00:00", percent).unwrap_or_else(|_| panic!("answers"));
        assert_eq!(after, json!({"at": null, "exact": true}));
        for malformed in [
            r#"{"levels": 0, "rounding": "nearest"}"#,
            r#"{"levels": 100, "rounding": "half"}"#,
            r#"{"levels": 100}"#,
            r#"{"levels": 100, "rounding": "up", "step": 0.01}"#,
        ] {
            // The refusal itself: a `JsError` is built only on a wasm target.
            assert!(super::parse_observation(malformed).is_err(), "{malformed}");
        }
    }

    /// Law 41 through the export: a qualified reference linked against a
    /// set of prodromes in `prices`'s shape reads the entity's function in
    /// its member, and a store or an entity the set lacks reads `∅`. A
    /// legacy member is no member, and a genesis named twice is refused.
    #[test]
    fn a_qualified_ref_links_through_the_stores_it_is_given() {
        let genesis = "ab".repeat(32);
        let other = "cd".repeat(32);
        let stores = json!([
            {"genesis": genesis, "env": {}, "functions": {"a": {"kind": "flat", "value": 0.4}}},
            {"genesis": null, "env": {}, "functions": {"a": {"kind": "flat", "value": 0.9}}},
        ])
        .to_string();
        let read = |store: &str, entity: &str| {
            let term = json!({"kind": "refIn", "store": store, "entity": entity}).to_string();
            let closed = super::link_in(&term, "{}", &stores).unwrap_or_else(|_| panic!("links"));
            super::fulfillment(&closed, "2026-11-01T00:00:00", "{}")
                .unwrap_or_else(|_| panic!("reads"))
        };
        assert_eq!(read(&genesis, "a"), Some(0.4), "the member's own function");
        assert_eq!(
            read(&genesis, "b"),
            None,
            "an entity the member lacks is absent"
        );
        assert_eq!(read(&other, "a"), None, "a store the set lacks is absent");
        let twice = json!([
            {"genesis": genesis, "env": {}, "functions": {}},
            {"genesis": genesis, "env": {}, "functions": {}},
        ]);
        assert!(
            super::parse_stores(&twice.to_string()).is_err(),
            "a genesis named twice"
        );
        assert!(super::parse_stores(r#"[{"genesis": "x", "env": {}, "functions": {}}]"#).is_err());
    }

    /// A store of changes begins at its `Genesis`: a change with no deps
    /// names no parents, and is no beginning.
    #[test]
    fn a_store_of_changes_begins_at_its_genesis() {
        let at = Datetime::new(2026, 9, 10, 9, 0, 0, 0).expect("an instant");
        let genesis: super::Object =
            Envelope::Genesis(mk_genesis("roadmap", &"0".repeat(32)).expect("a genesis"));
        let root = seal_hash(&genesis);
        let created = mk_created("a", at, "bassel", "a todo", "").expect("an event");
        let created: super::Object =
            Envelope::Change(mk_change(root.clone(), vec![], created).expect("a change"));
        let sent: Vec<Value> = [&genesis, &created]
            .into_iter()
            .map(|object| {
                json!({ "hash": seal_hash(object).as_str(), "text": canonical_envelope(object) })
            })
            .collect();
        let answer = super::verify_objects(&Value::from(sent).to_string())
            .unwrap_or_else(|_| panic!("verify answers"));
        let answer: Value = serde_json::from_str(&answer).expect("JSON");
        assert_eq!(answer["genesis"], json!([root.as_str()]));
        assert_eq!(answer["ok"], json!(true), "{answer}");
    }
}
