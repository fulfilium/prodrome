//! The exports, generic over a schema: what a page asks of a set of objects
//! whatever its events are.
//!
//! A schema says what its events are ([`prodrome::schema::Schema`]); this
//! crate reads a set of objects at one, and every function here answers JSON
//! text, or a refusal, a string, that a module's exports throw.
//!
//! ONE MODULE, ANY SCHEMAS. `#[wasm_bindgen]` exports no generic item, so a module
//! instantiates them with [`schema!`], once per schema, each as a JS class of
//! its own name: `new Reviews(objects)` reads the objects once, and every
//! method asks its question of what was read, or, `append`, writes into it.
//!
//! | method     | asks                                                         |
//! | ---------- | ------------------------------------------------------------ |
//! | `verify`   | §3: do these bytes hash to these names, form one DAG, and end at which tips |
//! | `tips`     | §3: the objects' tips, and each prodrome's heads by its genesis |
//! | `since`    | §3: what a replica holding these tips lacks, in causal order |
//! | `readings` | §6: each entity's registers, each its maximal writes          |
//! | `entries`  | §6.7, `priced` only: every entity as the folds see it        |
//! | `prices`   | §6.1, §6.4, `priced` only: the environment and every function |
//! | `next_changes` | §7, `priced` only: when each entity's observed price next changes |
//! | `append`   | §3: an event sealed into this replica, and the object to send |
//! | `sign`     | §3: a held object signed by a device key the host passes, and the object to send |
//! | `proven`   | §5: the objects a key registered to their actor signed       |
//! | `public_key` | §3, static: the public key of a device key's seed          |
//! | `declaration` | `declared` only: the schema it was admitted at          |
//!
//! EVERY READING IS TOTAL OVER WHAT THE REPLICA HOLDS. A print that is not
//! an object at the class's schema (one written under a constructor the
//! schema has since changed, one whose bytes do not hash to its name) is
//! left out of the history with everything resting on it, as a store's
//! largest down-set leaves it (SPEC law 40), and every other object reads
//! as it would had the print never arrived. So `tips`, `since`,
//! `readings`, `proven`, `entries`, `prices` and `next_changes` each answer
//! an object whose `excluded` is what was left out, `[{name, why,
//! resting}]` in name order: the print's name, why it is no object (`"failed
//! to parse: …"`, the parse's own refusal), and the names of the objects
//! resting on it through deps or parents. `verify` reports the same prints
//! as problems, by the same function ([`prodrome::dag::Dag::excluded`]).
//!
//! A SCHEMA DECLARED AS DATA ([`prodrome::declared`]) needs no crate of its
//! own: `schema!(Name, declared)` is a class whose constructor takes the
//! schema's text beside the objects, `new Name(schema, objects)`, admits it
//! (a refusal names the law that failed) and reads the objects at it.
//!
//! A schema crosses the boundary by [`Json`]: the field its events
//! name an entity by, each register's name, and each value's JSON, a function
//! of the value so that a reading's JSON is a function of the reading. A
//! schema with a row adds [`RowJson`], its reading's JSON; `priced`, its
//! `entries` and `prices`, asks for that row and the schema's
//! [`prodrome::schema::History`] and [`prodrome::schema::Bind`] too.
//! The todo schema has both, at any payload.
//!
//! A MODULE is a `cdylib` crate that depends on this one, on
//! `prodrome-core`, and on `wasm-bindgen` at the version this crate pins
//! (the version of the CLI it runs anyway); implements `Json` for each of
//! its schemas (and `RowJson` for each `priced` one); and says, once
//! per schema, `prodrome_wasm_exports::schema!(Reviews = Review);` or
//! `prodrome_wasm_exports::schema!(Todos = TodoEvent<Record>, priced);`. It
//! builds for `wasm32-unknown-unknown` and runs `wasm-bindgen` over the
//! result, as `nix build .#prodrome-wasm` does for `prodrome-wasm`, the
//! reference module. A module exports its classes and nothing of this
//! crate's own.

#[cfg(test)]
mod declared;
pub mod json;
#[cfg(test)]
mod review;
mod tags;
#[cfg(test)]
mod unread;
pub mod wire;

use std::collections::{BTreeMap, BTreeSet};

use std::sync::Arc;

use prodrome::dag::{Dag, Excluded, Finding, Unread};
pub use prodrome::declared::Declared;
use prodrome::declared::{Datum, Declaration, Slot};
use prodrome::event::{
    canonical_envelope, parents_of, parse_event, seal_hash, Envelope, Hash, TodoEvent,
};
use prodrome::fold::{Kind, Product};
use prodrome::fpl::{datetime_of, instant_of, iso, print_term, Instant};
use prodrome::literal::{Datetime, Value as Literal};
use prodrome::payload::Payload;
use prodrome::policy::Everything;
use prodrome::registers::Node;
use prodrome::schema::{self, Bind, History, Schema};
use prodrome::sign::{Proof, Registrar, Secret};
use prodrome::store::{MemoryStore, Replica as _};
use prodrome::todo::{self, TodoVocabulary};
use prodrome::view::{list_order, Entry};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use wire::{
    json_env, json_literal, json_next_change, json_reading, object, parse_keys, parse_moment,
    parse_names, parse_objects, parse_observation, parse_policy, strings, ObjectIn, Refusal,
};

/// What a schema says to cross this boundary, beside what it says to be
/// stored. Not a bound on [`Schema`]: JSON is this boundary's business, and a
/// schema a host only stores and reads natively owes it nothing.
///
/// A row names its entity by the fields of its key, each under its name: one
/// field for most schemas, and several for a key of several fields.
///
/// The names are read off the schema VALUE, so a schema declared at run
/// time names them as its declaration does, and a Rust one as its type does.
pub trait Json: Schema {
    /// The fields an event names its entity by, in printed order, and so the
    /// fields a row names it under: `["todo"]` for the todo schema.
    fn key_fields(schema: &Self::Vocabulary) -> Vec<&str>;

    /// The values of `key`'s fields, as JSON, in the order of
    /// [`Json::key_fields`].
    fn key_values(key: &Self::Key) -> Vec<Value>;

    /// A register's name, the key its reading is under.
    fn register(schema: &Self::Vocabulary, register: Self::Register) -> &str;

    /// The VALUE this event writes to `register`, one [`Schema::writes`]
    /// names, as JSON.
    ///
    /// A function of that value alone: two writes of one value answer the
    /// same JSON, so a reading's JSON is a function of the reading, the
    /// maximal values, and not of which writes carry them. Where a register's
    /// value is the whole event, as under the todo schema's discrete order,
    /// the event is the value.
    fn value(&self, register: Self::Register) -> Value;
}

/// What a schema with a [`schema::Row`] says more to cross this boundary: its
/// row's reading, as JSON.
pub trait RowJson: Json + schema::Row {
    /// A function of the reading alone, as [`Json::value`] is of a value.
    fn reading_json(reading: &Self::Reading) -> Value;
}

/// The todo schema, at any payload, crosses as it always has: each row names
/// its todo. Here and not in a host, which could not implement this crate's
/// trait for the core's type.
impl<P: Payload> Json for TodoEvent<P> {
    fn key_fields(_: &TodoVocabulary<P>) -> Vec<&str> {
        vec!["todo"]
    }

    fn key_values(key: &prodrome::event::TodoId) -> Vec<Value> {
        vec![json!(key.as_str())]
    }

    fn register(_: &TodoVocabulary<P>, kind: Kind) -> &str {
        kind.as_str()
    }

    /// A todo register's value is the whole event that wrote it
    /// (`todo::State`, `Spec`, `Content`), so its JSON is the event's §2
    /// literal, by `wire::json_literal`'s one mapping.
    fn value(&self, _kind: Kind) -> Value {
        json_literal(&Schema::to_value(self))
    }
}

/// A schema declared as data crosses as its declaration names it: each row
/// under its key field, each register under its name, and a value as its
/// register's order sees it (a list under inclusion as its set), each
/// datum as JSON: text, an enum's alternative and a reference as strings,
/// an integer as a number, an instant as ISO, a list as an array.
impl Json for Declared {
    fn key_fields(schema: &Declaration) -> Vec<&str> {
        vec![schema.key()]
    }

    fn key_values(key: &<Declared as Schema>::Key) -> Vec<Value> {
        vec![json!(key.as_ref())]
    }

    fn register(schema: &Declaration, slot: Slot) -> &str {
        schema.register_name(slot)
    }

    fn value(&self, slot: Slot) -> Value {
        self.written(slot).map_or(Value::Null, |datum| {
            datum_json(&self.schema().value(slot, datum).value())
        })
    }
}

fn datum_json(datum: &Datum) -> Value {
    match datum {
        Datum::Text(text) | Datum::Alternative(text) => json!(text),
        Datum::Reference(name) => json!(name.as_str()),
        Datum::Integer(int) => json!(int),
        Datum::Instant(at) => json!(iso(instant_of(*at))),
        Datum::List(items) => Value::Array(items.iter().map(datum_json).collect()),
    }
}

/// A todo's reading: its candidate outcomes as the environment spells one
/// (`null` open, a conflict as the array of its candidates), and the names
/// of its candidate records.
impl<P: Payload> RowJson for TodoEvent<P> {
    fn reading_json(reading: &todo::Reading) -> Value {
        json!({
            "outcome": json_reading(&reading.outcome),
            "content": names(&reading.content),
        })
    }
}

/// A schema's exports, in ONE module beside every other schema's: a JS class
/// named `$name` for the schema `$schema`, which must implement
/// [`Json`], and where `priced` is said, also [`prodrome::schema::History`],
/// [`prodrome::schema::Bind`] and [`RowJson`]. The invoking
/// crate depends on `wasm-bindgen` at this
/// crate's pinned version, which is the version of the `wasm-bindgen` CLI it
/// builds its module with anyway.
///
/// ```text
/// prodrome_wasm_exports::schema!(Reviews = my_host::Review);
/// prodrome_wasm_exports::schema!(Todos = prodrome::event::TodoEvent<my_host::Record>, priced);
/// prodrome_wasm_exports::schema!(Plugins, declared);
/// ```
///
/// A macro because `#[wasm_bindgen]` exports no generic item: each method is
/// one line, a call of the generic function in this crate of the same name,
/// so a schema costs its module the monomorphised functions and nothing else.
///
/// `new Reviews(objects)` reads the objects once (`[{hash, text}]`, see
/// [`Replica::of`]); every method asks its question of what was read.
/// A refusal is the JS exception, never a panic. Call `free()` when done, or
/// let the finaliser.
#[macro_export]
macro_rules! schema {
    ($name:ident = $schema:ty) => {
        $crate::schema!(@class $name = $schema);

        #[wasm_bindgen::prelude::wasm_bindgen]
        impl $name {
            /// The objects, `[{hash, text}]`, read once at this schema.
            #[wasm_bindgen(constructor)]
            pub fn new(objects: &str) -> Result<$name, wasm_bindgen::JsError> {
                $crate::Replica::of(objects)
                    .map($name)
                    .map_err(|refusal| wasm_bindgen::JsError::new(&refusal))
            }
        }
    };
    ($name:ident, declared) => {
        $crate::schema!(@class $name = $crate::Declared);

        #[wasm_bindgen::prelude::wasm_bindgen]
        impl $name {
            /// `schema`, a declared schema's text, admitted (a refusal
            /// names the law that failed), and the objects, `[{hash,
            /// text}]`, read once at it.
            #[wasm_bindgen(constructor)]
            pub fn new(schema: &str, objects: &str) -> Result<$name, wasm_bindgen::JsError> {
                $crate::Replica::declared(schema, objects)
                    .map($name)
                    .map_err(|refusal| wasm_bindgen::JsError::new(&refusal))
            }

            /// The schema it was admitted at: `{name, text, key,
            /// registers}`, its content name, canonical text, key field
            /// and register names.
            pub fn declaration(&self) -> Result<String, wasm_bindgen::JsError> {
                $crate::thrown($crate::declaration(&self.0))
            }
        }
    };
    (@class $name:ident = $schema:ty) => {
        #[doc = concat!("The exports of the schema `", stringify!($schema), "`.")]
        #[wasm_bindgen::prelude::wasm_bindgen]
        pub struct $name($crate::Replica<$schema>);

        #[wasm_bindgen::prelude::wasm_bindgen]
        impl $name {
            /// §3: do these bytes hash to these names, form one DAG, and end
            /// at which tips. Every row names its entity under the schema's
            /// key.
            pub fn verify(&self) -> Result<String, wasm_bindgen::JsError> {
                $crate::thrown($crate::verify(&self.0))
            }

            /// §3: the objects' tips, and each prodrome's heads by its
            /// genesis.
            pub fn tips(&self) -> Result<String, wasm_bindgen::JsError> {
                $crate::thrown($crate::tips(&self.0))
            }

            /// §3: what a replica holding `tips` (a JSON array of names)
            /// lacks, in causal order: the names to send it.
            pub fn since(&self, tips: &str) -> Result<String, wasm_bindgen::JsError> {
                $crate::thrown($crate::since(&self.0, tips))
            }

            /// §6: every entity's registers, each its maximal writes, at
            /// `at` (ISO, or `null` for everything) under `untrusted`.
            pub fn readings(
                &self,
                at: Option<String>,
                untrusted: &str,
            ) -> Result<String, wasm_bindgen::JsError> {
                $crate::thrown($crate::readings(&self.0, at, untrusted))
            }

            /// §3: `event`, its §2 print, sealed into this replica as a
            /// `Change` over its own fold, in the prodrome `genesis` names
            /// (`null`: the one last named, or the only one). Answers
            /// `{hash, objects}`: the object's name, and `[{hash, text}]`,
            /// what the host lacks of it, empty for a twin.
            pub fn append(
                &mut self,
                event: &str,
                genesis: Option<String>,
            ) -> Result<String, wasm_bindgen::JsError> {
                $crate::thrown($crate::append(&mut self.0, event, genesis))
            }

            /// §3: the held object `object` signed by the device key whose
            /// seed is `secret` (64 hex, kept by the host and passed per
            /// call), sealed into this replica as a `Signed`. Answers
            /// `{hash, objects}`, as `append` does.
            pub fn sign(
                &mut self,
                secret: &str,
                object: &str,
            ) -> Result<String, wasm_bindgen::JsError> {
                $crate::thrown($crate::sign(&mut self.0, secret, object))
            }

            /// §5: `{proven}`, the objects a key registered to their actor
            /// signed, key objects counted where one of `roots` (a JSON
            /// array of keys) signed them, or every one for `null`.
            pub fn proven(&self, roots: Option<String>) -> Result<String, wasm_bindgen::JsError> {
                $crate::thrown($crate::proven(&self.0, roots))
            }

            /// §3: the public key of the seed `secret`, to register.
            pub fn public_key(secret: &str) -> Result<String, wasm_bindgen::JsError> {
                $crate::thrown($crate::public_key(secret))
            }
        }
    };
    ($name:ident = $schema:ty, priced) => {
        $crate::schema!($name = $schema);

        #[wasm_bindgen::prelude::wasm_bindgen]
        impl $name {
            /// §6.7: every entity, as the folds see it at `at` (ISO, or
            /// `null` for the latest instant the objects stamp) under
            /// `untrusted`, in list order.
            pub fn entries(
                &self,
                at: Option<String>,
                untrusted: &str,
            ) -> Result<String, wasm_bindgen::JsError> {
                $crate::thrown($crate::entries(&self.0, at, untrusted))
            }

            /// §6.1 and §6.4, per prodrome: the environment, and every
            /// entity's fulfillment function.
            pub fn prices(
                &self,
                at: Option<String>,
                untrusted: &str,
            ) -> Result<String, wasm_bindgen::JsError> {
                $crate::thrown($crate::prices(&self.0, at, untrusted))
            }

            /// §7, per prodrome: when each entity's price, read through
            /// `observation` (`{levels, rounding}`), next changes after `at`,
            /// so a page schedules each row's redraw then.
            pub fn next_changes(
                &self,
                at: Option<String>,
                untrusted: &str,
                observation: &str,
            ) -> Result<String, wasm_bindgen::JsError> {
                $crate::thrown($crate::next_changes(&self.0, at, untrusted, observation))
            }
        }
    };
}

/// The name these bytes have: sha256 of the canonical print, exactly what
/// `seal_hash` takes and what `events/objects/<name>.py` holds — UTF-8, no
/// trailing newline.
pub fn name_of(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// An answer as an export returns it: a refusal is the exception a JS caller
/// catches, never a panic, which would leave the module poisoned for the rest
/// of the session.
pub fn thrown(answer: Result<String, Refusal>) -> Result<String, wasm_bindgen::JsError> {
    answer.map_err(|refusal| wasm_bindgen::JsError::new(&refusal))
}

pub fn printed(value: &Value) -> Result<String, Refusal> {
    serde_json::to_string(value).map_err(|e| format!("could not print the answer: {e}"))
}

/// One object as it was sent: the name it claims, the name its bytes have,
/// and whether the claim is a name at all.
struct Sent {
    hash: String,
    computed: String,
    named: bool,
    twice: bool,
}

/// A set of objects, read once at the schema `E`: what every export below
/// asks its question of. The bytes cross from JavaScript once, into the
/// [`Dag`]; nothing here holds them twice until the first [`append`], when
/// the replica becomes a [`MemoryStore`] holding them with their prints.
///
/// TOTAL OVER WHAT IT HOLDS. Its history is the largest down-set of the
/// objects ([`Dag::interior`]), as a store's is (SPEC law 40): a print that
/// is not an object at this schema (a migrated-away constructor, a tampered
/// file) leaves out itself and everything resting on it, and every other
/// object reads as it would had the print never arrived. What is left out
/// is [`Dag::excluded`], the function `verify` reports by, and every
/// reading answers it beside what it read.
pub struct Replica<E: Schema> {
    /// The schema the objects are read at.
    schema: E::Vocabulary,
    sent: Vec<Sent>,
    /// Every print sent, read at the schema, and every object written
    /// since: what `verify` reports on.
    prints: Arc<Dag<E>>,
    /// The history `prints` holds, its largest down-set: what every reading
    /// reads. The same value as `prints` while nothing is excluded.
    history: Arc<Dag<E>>,
    /// The history in causal order, or why this set has no honest reading.
    nodes: Result<Vec<Node<E>>, Refusal>,
    /// What the history leaves out of `prints`, and why. An append writes
    /// over the history, so nothing it writes rests on what is left out.
    excluded: Vec<Excluded>,
    /// The store an append writes into, once one has.
    store: Option<MemoryStore<E>>,
}

impl<E: Schema> Replica<E> {
    /// `objects` is `[{hash, text}]`: every object's claimed name beside the
    /// exact canonical print that name is a hash of. Refused only where it is
    /// not that shape; what the objects are is [`verify`]'s to say.
    pub fn of(objects: &str) -> Result<Replica<E>, Refusal>
    where
        E::Vocabulary: Default,
    {
        Replica::at(E::Vocabulary::default(), objects)
    }

    /// [`Replica::of`], read at the schema `schema`, a value: a declared
    /// schema admitted at run time, or a Rust schema's unit.
    ///
    /// # Errors
    ///
    /// [`Replica::of`]'s: `objects` is not `[{hash, text}]`.
    pub fn at(schema: E::Vocabulary, objects: &str) -> Result<Replica<E>, Refusal> {
        let mut seen = BTreeSet::new();
        let mut sent = Vec::new();
        let mut prints = Vec::new();
        for ObjectIn { hash, text } in parse_objects(objects)? {
            let computed = name_of(&text);
            let twice = !seen.insert(hash.clone());
            let named = match Hash::new(hash.clone()) {
                Ok(name) => {
                    if !twice {
                        prints.push((name, Ok(text.into_bytes())));
                    }
                    true
                }
                Err(_) => false,
            };
            sent.push(Sent {
                hash,
                computed,
                named,
                twice,
            });
        }
        let prints = Arc::new(Dag::from_prints(&schema, prints));
        let excluded = prints.excluded();
        let history = if excluded.is_empty() {
            Arc::clone(&prints)
        } else {
            Arc::new(prints.interior())
        };
        let nodes = Replica::order(&sent, &history);
        Ok(Replica {
            schema,
            sent,
            prints,
            history,
            nodes,
            excluded,
            store: None,
        })
    }

    /// The history in causal order. A claimed name that is no name at all
    /// is refused, since no print is held under it to leave out: every
    /// reading refuses where [`verify`] reports it.
    fn order(sent: &[Sent], history: &Dag<E>) -> Result<Vec<Node<E>>, Refusal> {
        if let Some(Sent { hash, .. }) = sent.iter().find(|sent| !sent.named) {
            return Err(format!(
                "object {hash} does not hash to its own name (tampered/corrupt)"
            ));
        }
        history.nodes().map_err(|e| e.to_string())
    }

    /// The schema the objects are read at.
    pub fn schema(&self) -> &E::Vocabulary {
        &self.schema
    }

    /// The history, read, with what it leaves out.
    ///
    /// # Errors
    ///
    /// A claimed name that is no name ([`Replica::order`]).
    pub fn read(&self) -> Result<Read<'_, E>, Refusal> {
        self.nodes.as_ref().map_err(Clone::clone).map(|nodes| Read {
            dag: &self.history,
            nodes,
            excluded: &self.excluded,
        })
    }
}

impl Replica<Declared> {
    /// [`Replica::at`] a schema declared as data: `schema` is its text,
    /// admitted first.
    ///
    /// # Errors
    ///
    /// The law the schema does not keep, named; then [`Replica::at`]'s.
    pub fn declared(schema: &str, objects: &str) -> Result<Replica<Declared>, Refusal> {
        let schema = Declaration::admit(schema).map_err(|refusal| format!("schema: {refusal}"))?;
        Replica::at(schema, objects)
    }
}

/// What a declared replica was admitted at: its content name, canonical
/// text, key field and register names.
///
/// # Errors
///
/// None in practice: [`printed`]'s, should JSON not print.
pub fn declaration(replica: &Replica<Declared>) -> Result<String, Refusal> {
    let schema = &replica.schema;
    printed(&object(vec![
        ("name", json!(schema.name().as_str())),
        ("text", json!(schema.text())),
        ("key", json!(schema.key())),
        (
            "registers",
            strings(
                schema
                    .registers()
                    .map(|slot| schema.register_name(slot).to_owned()),
            ),
        ),
    ]))
}

// --- §3: append ------------------------------------------------------------------

/// `event`, its §2 print at the schema `E`, sealed into `replica` as a
/// `Change` (SPEC §3): its deps from the replica's own fold, a twin of an
/// event it holds answered and nothing written, by the core's
/// [`MemoryStore`], so the object is byte for byte the one the host's store
/// would write holding what the replica holds. `genesis` names the prodrome
/// to write into, and stays named; `None` keeps the one named before, or,
/// never named, the only one.
///
/// Answers `{hash, objects}`: the object's name, and the objects to send
/// the host, `[{hash, text}]` as [`Replica::of`] takes them, which a store
/// adopts verified (`EventStore::adopt_objects`, or `receive`): the new
/// object, or nothing for a twin. Every reading after it reads it.
///
/// # Errors
///
/// Objects with no honest reading, as every reading refuses them; an event
/// or a genesis that does not parse; an append the store would refuse.
pub fn append<E: Schema>(
    replica: &mut Replica<E>,
    event: &str,
    genesis: Option<String>,
) -> Result<String, Refusal> {
    replica.read()?;
    let event: E = parse_event(&replica.schema, event).map_err(|e| format!("event: {e}"))?;
    let genesis = genesis
        .map(|genesis| Hash::new(genesis).map_err(|e| format!("genesis: {e}")))
        .transpose()?;
    write(replica, genesis, |store| {
        store.append(event).map_err(|e| e.to_string())
    })
}

/// `secret`'s signature of the object `object` this replica holds, sealed
/// into it as a `Signed` (SPEC §3). `secret` is a device key's seed, 64
/// lowercase hex: the host keeps it where it decides (a phone in its own
/// storage) and passes it to each call, and the module holds no key.
/// Signing is deterministic, so signing again answers the same object and
/// nothing to send.
///
/// Answers `{hash, objects}`, as [`append`] does.
///
/// # Errors
///
/// Objects with no honest reading; a secret that is not a seed; an object
/// name that is not one, or that this replica does not hold.
pub fn sign<E: Schema>(
    replica: &mut Replica<E>,
    secret: &str,
    object: &str,
) -> Result<String, Refusal> {
    let read = replica.read()?;
    let object = Hash::new(object).map_err(|e| format!("object: {e}"))?;
    if read.dag.get(&object).is_none() {
        return Err(format!("object: {object} is not held"));
    }
    let signed = Envelope::<E>::Signed(
        Secret::new(secret)
            .map_err(|e| format!("secret: {e}"))?
            .sign(&object),
    );
    let (name, print) = (seal_hash(&signed), canonical_envelope(&signed).into_bytes());
    write(replica, None, |store| {
        store
            .receive([name.clone()].into(), &|wanted| {
                (*wanted == name).then(|| print.clone())
            })
            .map_err(|e| e.to_string())?;
        Ok(name.clone())
    })
}

/// The public key of the seed `secret`: what a root registers for a device
/// in a `KeyAdded`.
///
/// # Errors
///
/// A secret that is not a seed.
pub fn public_key(secret: &str) -> Result<String, Refusal> {
    Secret::new(secret)
        .map(|secret| secret.public().into_string())
        .map_err(|e| format!("secret: {e}"))
}

/// The objects carrying an event that these prove (SPEC §5): signed by a
/// key registered to their actor, beneath every revocation of it. `roots`
/// is a JSON array of root keys, the key objects a root signed counted, or
/// `null` for every key object, as `verify` counts them. Answers `{proven}`,
/// their names in causal order.
///
/// # Errors
///
/// Objects with no honest reading; roots that are not keys.
pub fn proven<E: Schema>(replica: &Replica<E>, roots: Option<String>) -> Result<String, Refusal> {
    let read = replica.read()?;
    let registrar = match roots {
        None => Registrar::Anyone,
        Some(roots) => Registrar::Roots(parse_keys("roots", &roots)?.into_iter().collect()),
    };
    let proof = Proof::of(read.dag, &registrar);
    let proven = read
        .nodes
        .iter()
        .map(Node::name)
        .filter(|name| proof.proves(name));
    read.answer(vec![("proven", names(proven))])
}

/// Seal what `write` writes into `replica`'s store, which holds what the
/// replica holds, and answer `{hash, objects}`: the object's name and,
/// unless the host was sent it already or it was a twin, its print.
fn write<E: Schema>(
    replica: &mut Replica<E>,
    genesis: Option<Hash>,
    write: impl FnOnce(&MemoryStore<E>) -> Result<Hash, Refusal>,
) -> Result<String, Refusal> {
    let mut store = if let Some(store) = replica.store.take() {
        store
    } else {
        let dag = &replica.history;
        let store = MemoryStore::at(replica.schema.clone());
        store
            .receive(dag.tips(), &|name| {
                dag.get(name)
                    .map(|object| canonical_envelope(object).into_bytes())
            })
            .map_err(|e| e.to_string())?;
        store
    };
    if let Some(genesis) = genesis {
        store = store.in_genesis(genesis);
    }
    // The store's objects are shared with this replica's history; let go of
    // them, so the write extends them where they are rather than copying.
    let one = Arc::ptr_eq(&replica.prints, &replica.history);
    replica.history = Arc::new(Dag::from_iter([]));
    if one {
        replica.prints = Arc::clone(&replica.history);
    }
    let appended = write(&store);
    let history = store.held().map_err(|e| e.to_string())?.dag;
    replica.store = Some(store);
    replica.nodes = history.nodes().map_err(|e| e.to_string());
    replica.history = history;
    if one {
        replica.prints = Arc::clone(&replica.history);
    }
    let name = appended?;
    if let Some(object) = replica.history.get(&name).filter(|_| !one) {
        Arc::make_mut(&mut replica.prints).extend([(name.clone(), object.clone())]);
    }
    let mut objects = Vec::new();
    let sent = replica.sent.iter().any(|sent| sent.hash == name.as_str());
    if let Some(object) = replica.history.get(&name).filter(|_| !sent) {
        replica.sent.push(Sent {
            hash: name.as_str().to_owned(),
            computed: name.as_str().to_owned(),
            named: true,
            twice: false,
        });
        let text = canonical_envelope(object);
        objects.push(json!({ "hash": name.as_str(), "text": text }));
    }
    printed(&object(vec![
        ("hash", json!(name.as_str())),
        ("objects", Value::Array(objects)),
    ]))
}

/// The history, rehashed, parsed and put in causal order — what every fold
/// reads — and what it leaves out, which every answer says.
pub struct Read<'r, E> {
    pub dag: &'r Dag<E>,
    pub nodes: &'r [Node<E>],
    pub excluded: &'r [Excluded],
}

impl<E: Schema> Read<'_, E> {
    /// This reading, for an answer with no room for what it excludes: such
    /// an answer refuses, naming the first name left out, and never drops
    /// it silently.
    ///
    /// # Errors
    ///
    /// The first name the history leaves out, and why.
    pub fn whole(self) -> Result<Self, Refusal> {
        match self.excluded.first() {
            None => Ok(self),
            Some(Excluded {
                name, why: None, ..
            }) => Err(format!("missing object {}", name.as_str())),
            Some(Excluded {
                name,
                why: Some(why),
                ..
            }) => Err(format!("object {} {}", name.as_str(), unread(name, why))),
        }
    }

    /// An answer of this reading: `fields`, and `excluded`, what the
    /// history leaves out, `[{name, why, resting}]` in name order, which no
    /// reading answers without. `why` is the parse's refusal of the print
    /// (or that it does not hash to its name), or `"not held"` for a parent
    /// no print is; `resting` the objects it leaves out with it, each
    /// resting on it through deps or parents.
    fn answer(&self, mut fields: Vec<(&str, Value)>) -> Result<String, Refusal> {
        let excluded = self
            .excluded
            .iter()
            .map(|out| {
                object(vec![
                    ("name", json!(out.name.as_str())),
                    (
                        "why",
                        json!(out
                            .why
                            .as_ref()
                            .map_or_else(|| "not held".to_owned(), |why| unread(&out.name, why))),
                    ),
                    ("resting", names(&out.resting)),
                ])
            })
            .collect();
        fields.push(("excluded", Value::Array(excluded)));
        printed(&object(fields))
    }

    /// The events, in the linearisation's order — merges dropped, since a
    /// merge is structure and carries no event to fold.
    pub fn events(&self) -> impl Iterator<Item = &E> {
        self.nodes.iter().filter_map(Node::event)
    }

    /// The moment to fold at. `None` means "everything the chain holds", which
    /// is the LATEST INSTANT THE CHAIN ITSELF STAMPS and never the host's
    /// clock: a core that read a clock would let one decide who wins, which is
    /// the thing §1 forbids. A caller that means "now" says so.
    pub fn moment(&self, at: Option<Instant>) -> Result<Datetime, Refusal> {
        let instant = match at {
            Some(instant) => instant,
            None => self
                .events()
                .map(|event| instant_of(event.at()))
                .max()
                .unwrap_or_else(|| {
                    prodrome::fpl::parse_iso("0001-01-01T00:00:00").expect("a real instant")
                }),
        };
        datetime_of(instant).map_err(|e| format!("at: {e}"))
    }
}

// --- §3: verify --------------------------------------------------------------

/// One object's row on the chain page: what it claims, what this core
/// computed, and where the walk found it.
struct Row {
    hash: String,
    computed: String,
    parents: Vec<Hash>,
    kind: String,
    key: Vec<Value>,
    actor: String,
    at: String,
    problem: String,
    reachable: bool,
    depth: Option<usize>,
    root: bool,
}

impl Row {
    fn blank(hash: String, computed: String) -> Row {
        Row {
            hash,
            computed,
            parents: Vec::new(),
            kind: String::new(),
            key: Vec::new(),
            actor: String::new(),
            at: String::new(),
            problem: String::new(),
            reachable: false,
            depth: None,
            root: false,
        }
    }

    fn hash_ok(&self) -> bool {
        self.hash == self.computed
    }

    fn tampered(&self) -> String {
        format!(
            "does not hash to its own name — this browser computed {}",
            self.computed
        )
    }

    /// The four fields an event gives its row, read from the object's own
    /// bytes: its constructor, its entity, its writer and its instant. An
    /// object with no event leaves them "" and is kinded by its name.
    fn describe<E: Json>(&mut self, envelope: &Envelope<E>) {
        self.parents = parents_of(envelope);
        self.root = matches!(
            envelope,
            Envelope::Genesis(_) | Envelope::Sealed { prev: None, .. }
        );
        match envelope.event() {
            Some(event) => {
                self.kind = match event.to_value() {
                    Literal::Call(call) => call.name,
                    _ => String::new(),
                };
                self.key = E::key_values(event.key());
                event.actor().as_str().clone_into(&mut self.actor);
                self.at = iso(instant_of(event.at()));
            }
            None => envelope.name().clone_into(&mut self.kind),
        }
    }

    fn json<E: Json>(&self, schema: &E::Vocabulary) -> Value {
        let mut fields = vec![
            ("hash", json!(self.hash)),
            ("computed", json!(self.computed)),
            ("hashOk", json!(self.hash_ok())),
            ("reachable", json!(self.reachable)),
            ("depth", json!(self.depth)),
            ("kind", json!(self.kind)),
        ];
        fields.extend(keyed::<E>(schema, &self.key));
        fields.extend(vec![
            ("actor", json!(self.actor)),
            ("at", json!(self.at)),
            ("problem", json!(self.problem)),
        ]);
        object(fields)
    }
}

/// Each of a row's key fields under its name, with its value in `values`,
/// or `""` for a row whose object carries no event.
fn keyed<'s, E: Json>(schema: &'s E::Vocabulary, values: &[Value]) -> Vec<(&'s str, Value)> {
    E::key_fields(schema)
        .into_iter()
        .enumerate()
        .map(|(at, field)| (field, values.get(at).cloned().unwrap_or_else(|| json!(""))))
        .collect()
}

/// §3's `verify`, for the set of objects a browser was handed.
///
/// The objects are read as one [`prodrome::dag::Dag`], and the answer's `tips`
/// are its tips: nobody has to tell a replica where its heads are.
///
/// The findings are the `Dag`'s, less the ones about a DIRECTORY, which a set
/// in memory has none of. A page's own wording stands where it had one: an
/// object whose bytes do not hash to its name, and a parent that was not sent.
/// The walk from the tips gives each row its depth, and what it does not
/// reach (only an object that is not one, or one caught in a cycle) is said.
pub fn verify<E: Json>(replica: &Replica<E>) -> Result<String, Refusal> {
    let dag = &replica.prints;
    let mut problems: Vec<String> = Vec::new();
    let mut rows: Vec<Row> = Vec::new();
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    for sent in &replica.sent {
        if sent.twice {
            problems.push(format!("object {} was sent twice", sent.hash));
            continue;
        }
        index.insert(sent.hash.clone(), rows.len());
        let mut row = Row::blank(sent.hash.clone(), sent.computed.clone());
        if !sent.named {
            row.problem = row.tampered();
            problems.push(format!("object {} {}", sent.hash, row.problem));
        }
        rows.push(row);
    }
    for (name, object) in dag.objects() {
        rows[index[name.as_str()]].describe(object);
    }
    for finding in dag.verify(&Everything) {
        match finding {
            Finding::Unread { name, why } => {
                let row = &mut rows[index[name.as_str()]];
                row.problem = match why {
                    Unread::Tampered(_) => row.tampered(),
                    _ => unread(&name, &why),
                };
                problems.push(format!("object {} {}", name.as_str(), row.problem));
            }
            Finding::Broken { at, why: None } => problems.push(format!(
                "object {} is named as a parent but was not sent",
                at.as_str()
            )),
            Finding::Broken { .. } => {}
            other => problems.push(other.to_string()),
        }
    }

    let tips: Vec<String> = dag
        .tips()
        .into_iter()
        .map(|tip| tip.as_str().to_owned())
        .collect();
    // Breadth first from every tip at once, so `depth` is the distance from
    // the nearest tip and the order stays tips-first the way the page reads
    // it.
    let mut order: Vec<usize> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<(String, usize)> = tips.iter().map(|name| (name.clone(), 0)).collect();
    let mut cursor = 0;
    while cursor < queue.len() {
        let (name, depth) = queue[cursor].clone();
        cursor += 1;
        let Some(at) = index.get(&name).copied().filter(|_| seen.insert(name)) else {
            continue;
        };
        rows[at].reachable = true;
        rows[at].depth = Some(depth);
        order.push(at);
        for parent in rows[at].parents.clone() {
            queue.push((parent.as_str().to_owned(), depth + 1));
        }
    }

    // A genesis object says so itself: an object that did not parse names no
    // parents either, and calling that a beginning would turn a tampered tip
    // into a clean chain of one. A change with no deps is no beginning.
    let genesis: Vec<String> = order
        .iter()
        .filter(|at| rows[**at].root && rows[**at].problem.is_empty())
        .map(|at| rows[*at].hash.clone())
        .collect();
    if genesis.is_empty() && problems.is_empty() && !rows.is_empty() {
        problems.push(
            "the walk ended without reaching a genesis object (an object with no parents)"
                .to_owned(),
        );
    }

    let linearisation: Vec<String> = dag
        .linearise()
        .unwrap_or_default()
        .iter()
        .map(|name| name.as_str().to_owned())
        .collect();

    let orphans: Vec<usize> = (0..rows.len()).filter(|at| !rows[*at].reachable).collect();
    for at in &orphans {
        problems.push(format!(
            "object {} is not reachable from any head",
            rows[*at].hash
        ));
        if rows[*at].problem.is_empty() {
            rows[*at].problem = "not reachable from any head by its parents".to_owned();
        }
    }

    let listed: Vec<usize> = order.iter().copied().chain(orphans).collect();
    let verified = listed.iter().filter(|at| rows[**at].hash_ok()).count();
    printed(&object(vec![
        ("tips", strings(tips)),
        (
            "objects",
            Value::Array(
                listed
                    .iter()
                    .map(|at| rows[*at].json::<E>(&replica.schema))
                    .collect(),
            ),
        ),
        ("walked", Value::from(order.len())),
        ("verified", Value::from(verified)),
        ("genesis", strings(genesis)),
        ("ok", Value::Bool(problems.is_empty())),
        ("problems", strings(problems)),
        ("linearisation", strings(linearisation)),
    ]))
}

// --- §3: where a replica stands ------------------------------------------------

/// Why the print under `name` is not an object, as a page words it.
fn unread(name: &Hash, why: &Unread) -> String {
    match why {
        Unread::Tampered(computed) => format!(
            "does not hash to its own name (tampered/corrupt): its bytes hash to {}",
            computed.as_str()
        ),
        _ => format!("failed to parse: {}", why.refusal(name)),
    }
}

fn names<'n>(names: impl IntoIterator<Item = &'n Hash>) -> Value {
    strings(names.into_iter().map(|name| name.as_str().to_owned()))
}

/// The objects' TIPS, those no object names as a parent, and each prodrome's
/// HEADS, its own tips, by the name of its genesis (a legacy store's by its
/// least root): the history's, beside what it leaves out.
pub fn tips<E: Schema>(replica: &Replica<E>) -> Result<String, Refusal> {
    let read = replica.read()?;
    let dag = read.dag;
    let heads = dag
        .geneses()
        .into_iter()
        .map(|genesis| {
            let heads = names(&dag.tips_in(&dag.key(&genesis)));
            (genesis.into_string(), heads)
        })
        .collect();
    read.answer(vec![
        ("tips", names(&dag.tips())),
        ("heads", Value::Object(heads)),
    ])
}

/// The objects a replica that holds `tips` (a JSON array of names) and
/// everything they rest on does not, in causal order: what to send it. A tip
/// these objects lack names nothing they hold, so it holds back nothing.
/// Names only: the caller holds the bytes it sent.
pub fn since<E: Schema>(replica: &Replica<E>, tips: &str) -> Result<String, Refusal> {
    let read = replica.read()?;
    let held = read.dag.closure(parse_names("tips", tips)?);
    let since = read
        .nodes
        .iter()
        .map(Node::name)
        .filter(|name| !held.contains(*name));
    read.answer(vec![("since", names(since))])
}

// --- §6: the registers, read ---------------------------------------------------

/// Every entity's registers, read: each register [`Schema::registers`] names,
/// as its READING (design §3), the writes whose values are maximal under its
/// type's order, each `{hash, value}`. One is a value; more is a conflict;
/// none is unwritten. A schema with no valuation is read whole by this.
///
/// At `at` (ISO, or `null` for every write the objects hold) and under
/// `untrusted`, a list of actor names read as the reference policy (§5), or
/// `{untrusted, roots}`, that policy held to the actors' keys under the root
/// keys `roots` ([`wire::WirePolicy`]); so for every reading below. Per
/// entity in (genesis, key) order: `genesis` is the prodrome's, `null` for a
/// legacy one, and the entity's key is under the schema's [`Json::key_fields`].
pub fn readings<E: Json>(
    replica: &Replica<E>,
    at: Option<String>,
    untrusted: &str,
) -> Result<String, Refusal> {
    let read = replica.read()?;
    let nodes = read.nodes;
    let policy = parse_policy(untrusted, read.dag)?;
    let at = parse_moment("at", at)?;
    let state = prodrome::registers::fold(nodes);
    let mut rows = Vec::new();
    for (genesis, prodrome) in state.prodromes() {
        for (key, stream) in prodrome {
            let registers = prodrome::fold::read(stream, at, &policy);
            let readings = E::registers(&replica.schema)
                .into_iter()
                .map(|register| {
                    let reading = registers
                        .reading(register)
                        .into_iter()
                        .map(|stamp| {
                            json!({
                                "hash": stamp.name.as_str(),
                                "value": stamp.event.value(register),
                            })
                        })
                        .collect();
                    (
                        E::register(&replica.schema, register).to_owned(),
                        Value::Array(reading),
                    )
                })
                .collect();
            rows.push(object(
                vec![("genesis", json!(genesis.as_ref().map(Hash::as_str)))]
                    .into_iter()
                    .chain(keyed::<E>(&replica.schema, &E::key_values(key)))
                    .chain([("registers", Value::Object(readings))])
                    .collect::<Vec<_>>(),
            ));
        }
    }
    read.answer(vec![("readings", Value::Array(rows))])
}

// --- §6.7 and §7: a schema's price -----------------------------------------------

/// The moment a priced reading is taken at: `at` (ISO), or `None` for the
/// latest instant the objects stamp ([`Read::moment`]).
fn moment<E: Schema>(read: &Read<'_, E>, at: Option<String>) -> Result<Datetime, Refusal> {
    read.moment(parse_moment("at", at)?)
}

fn json_entry<E: RowJson>(schema: &E::Vocabulary, entry: &Entry<E>) -> Value {
    let head: Vec<(&str, Value)> =
        std::iter::once(("genesis", json!(entry.genesis.as_ref().map(Hash::as_str))))
            .chain(keyed::<E>(schema, &E::key_values(&entry.key)))
            .collect();
    object(
        head.into_iter()
            .chain([
                ("reading", E::reading_json(&entry.reading)),
                (
                    "claim",
                    entry.claim.as_ref().map_or(Value::Null, E::reading_json),
                ),
                ("unconfirmed", json!(entry.confidence.is_provisional())),
                (
                    "value",
                    match entry.value() {
                        Ok(Some(value)) => json!(value),
                        Ok(None) => json!("absent"),
                        Err(_) => Value::Null,
                    },
                ),
                ("unlinked", json!(entry.unlinked().map(ToString::to_string))),
                (
                    "conflicts",
                    Value::Object(
                        entry
                            .conflicts
                            .iter()
                            .map(|(register, writes)| {
                                (E::register(schema, *register).to_owned(), names(writes))
                            })
                            .collect(),
                    ),
                ),
                ("spec", json!(print_term(entry.spec()))),
                ("stream", names(&entry.stream)),
            ])
            .collect(),
    )
}

/// §6.7: every entity the objects mention, as the folds see it at `at` under
/// `untrusted`, in §6.7's list order (most urgent first, then every row with
/// no number, each by genesis and key). A row is its entity's confirmed
/// reading, the claimed one where it disputes it, whether it is provisional,
/// its price (`value` a number, `"absent"` for `∅`, `null` where it does not
/// link and `unlinked` says why), its conflicts by register, its function as
/// its §2 print, and its stream.
pub fn entries<E: RowJson + History + Bind>(
    replica: &Replica<E>,
    at: Option<String>,
    untrusted: &str,
) -> Result<String, Refusal> {
    let read = replica.read()?;
    let policy = parse_policy(untrusted, read.dag)?;
    let moment = moment(&read, at)?;
    let mut rows =
        prodrome::view::entries(read.nodes, moment, &policy).map_err(|e| e.to_string())?;
    rows.sort_by(list_order);
    read.answer(vec![
        ("at", Value::String(iso(instant_of(moment)))),
        (
            "entries",
            Value::Array(
                rows.iter()
                    .map(|entry| json_entry(&replica.schema, entry))
                    .collect(),
            ),
        ),
    ])
}

/// §6.1 and §6.4, per prodrome: the environment FPL's terms read at `at`
/// under `untrusted`, and every entity's fulfillment function (`absent`
/// where it has none), in `json.rs`'s term shape: what `link`,
/// `fulfillment`, `explain` and `series_knots` take, so a page draws a price
/// without a second reading of it.
pub fn prices<E: History + Bind>(
    replica: &Replica<E>,
    at: Option<String>,
    untrusted: &str,
) -> Result<String, Refusal> {
    let read = replica.read()?;
    let policy = parse_policy(untrusted, read.dag)?;
    let moment = moment(&read, at)?;
    let now = instant_of(moment);
    let state = prodrome::registers::fold(read.nodes);
    let mut prices = Vec::new();
    for (genesis, prodrome) in state.prodromes() {
        let functions = prodrome::fold::flatten(prodrome, now, &policy).map_err(|e| e.0)?;
        let functions = prodrome::fold::link_specs(&functions, prodrome.keys())
            .into_iter()
            .map(|(key, term)| (key, json::to_json(&term)))
            .collect();
        prices.push(object(vec![
            ("genesis", json!(genesis.as_ref().map(Hash::as_str))),
            (
                "env",
                json_env(&prodrome::fold::env(prodrome, now, &policy)),
            ),
            ("functions", Value::Object(functions)),
        ]));
    }
    read.answer(vec![
        ("at", Value::String(iso(now))),
        ("prices", Value::Array(prices)),
    ])
}

/// §7, per prodrome: when each entity's price, read through `observation`
/// ([`wire::parse_observation`]), next changes after `at` under `untrusted`:
/// `{at, changes: [{genesis, next: {<key>: {at, exact}}}]}`, a row's `at`
/// `null` for never ([`wire::json_next_change`]), and a row `null` where its
/// function does not link. The price is the one `entries` shows: the
/// entity's function linked against its prodrome's, under the environment
/// at `at`. A page redraws a row at its `at` and not on a ticking clock.
///
/// # Errors
///
/// What `prices` refuses, and an observation [`wire::parse_observation`]
/// refuses.
pub fn next_changes<E: History + Bind>(
    replica: &Replica<E>,
    at: Option<String>,
    untrusted: &str,
    observation: &str,
) -> Result<String, Refusal> {
    let read = replica.read()?;
    let policy = parse_policy(untrusted, read.dag)?;
    let observation = parse_observation(observation)?;
    let now = instant_of(moment(&read, at)?);
    let state = prodrome::registers::fold(read.nodes);
    let mut changes = Vec::new();
    for (genesis, prodrome) in state.prodromes() {
        let functions = prodrome::fold::flatten(prodrome, now, &policy).map_err(|e| e.0)?;
        let linkable = prodrome::fold::link_specs(&functions, prodrome.keys());
        let env = prodrome::fold::env(prodrome, now, &policy);
        let next = prodrome
            .keys()
            .map(|key| {
                let spec = functions
                    .get(key)
                    .cloned()
                    .unwrap_or_else(prodrome::fpl::mk_absent);
                let next = prodrome::fpl::link(&spec, &linkable).map_or(Value::Null, |closed| {
                    json_next_change(prodrome::observe::next_change(
                        &closed,
                        now,
                        &env,
                        observation,
                    ))
                });
                (key.as_ref().to_owned(), next)
            })
            .collect();
        changes.push(object(vec![
            ("genesis", json!(genesis.as_ref().map(Hash::as_str))),
            ("next", Value::Object(next)),
        ]));
    }
    read.answer(vec![
        ("at", Value::String(iso(now))),
        ("changes", Value::Array(changes)),
    ])
}
