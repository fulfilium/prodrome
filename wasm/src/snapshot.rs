//! Every export's answer over `conformance/dag.py`'s seed-1 DAG, byte for byte.

use std::fmt::Write as _;

use prodrome::literal::{parse_literal, Open, Value as Literal};
use serde_json::{json, Value};

const DAGS: &str = include_str!("../../conformance/dag.py");
const SNAPSHOT: &str = include_str!("../snapshots/dag-1.txt");
const SEED: i64 = 1;
const UNTRUSTED: &str = r#"["triage"]"#;
/// Every constructor, over the DAG's todos: two links, a reference, a
/// recurrence and schedules under the operators `normalize` pushes through.
const COMPOSITE: &str = r#"{"kind": "conj", "p": -4.0, "terms": [
  {"kind": "after", "event": "alpha", "anchor": "2026-09-10T00:00:00", "needsHours": 48.0,
   "term": {"kind": "decay", "start": 0.9, "end": 0.1, "endDate": "2026-09-30T00:00:00", "leadUpHours": 240.0},
   "pending": {"kind": "absent"}},
  {"kind": "after", "event": "beta", "anchor": "2026-09-20T00:00:00",
   "term": {"kind": "shift", "deltaHours": -12.0, "term": {"kind": "piecewise",
     "head": {"kind": "flat", "value": 0.4},
     "pieces": [{"at": "2026-10-01T00:00:00", "term": {"kind": "curve", "points": [
       {"at": "2026-10-01T00:00:00", "value": 0.2, "label": "go"},
       {"at": "2026-10-20T00:00:00", "value": 0.8}]}}]}},
   "pending": {"kind": "flat", "value": 0.6}},
  {"kind": "gate", "gate": {"kind": "ref", "todo": "gamma"},
   "body": {"kind": "importance", "w": 2.0, "term": {"kind": "piecewise",
     "head": {"kind": "absent"},
     "pieces": [{"at": "2026-09-05T00:00:00", "term": {"kind": "flat", "value": 0.7}}]}}},
  {"kind": "offsetBy", "delta": {"kind": "offset", "delta": -0.2, "term": {"kind": "ref", "todo": "beta"}},
   "term": {"kind": "within", "windowHours": 72.0, "p": -2.0, "term": {"kind": "ref", "todo": "alpha"}}},
  {"kind": "recur", "todo": "gamma", "anchor": "2026-09-01T00:00:00",
   "term": {"kind": "periodic", "periodHours": 168.0, "anchor": "2026-09-01T00:00:00",
     "term": {"kind": "decay", "start": 0.98, "end": 0.3, "endDate": "2026-09-08T00:00:00", "leadUpHours": 120.0}},
   "pending": {"kind": "flat", "value": 0.3}}]}"#;
/// Two lines that cross and a schedule that starts absent: exact, with a knot
/// at the crossing.
const LEAST: &str = r#"{"kind": "least", "terms": [
  {"kind": "decay", "start": 0.9, "end": 0.1, "endDate": "2026-10-15T00:00:00", "leadUpHours": 720.0},
  {"kind": "flat", "value": 0.5},
  {"kind": "piecewise", "head": {"kind": "absent"},
   "pieces": [{"at": "2026-09-10T00:00:00", "term": {"kind": "curve", "points": [
     {"at": "2026-09-10T00:00:00", "value": 0.8}, {"at": "2026-10-10T00:00:00", "value": 0.2}]}}]}]}"#;
const NOWS: [&str; 3] = [
    "2026-08-15T00:00:00",
    "2026-09-20T12:00:00",
    "2026-10-30T00:00:00",
];

fn ok<T, E>(answer: Result<T, E>) -> T {
    answer.unwrap_or_else(|_| panic!("the export refused"))
}

fn objects() -> Vec<(String, String)> {
    let dags = parse_literal(DAGS, &Open).expect("dag.py parses");
    let text = |value: &Literal| value.as_str().expect("a string").to_owned();
    dags.as_call()
        .and_then(|root| root.field("dags"))
        .and_then(Literal::as_tuple)
        .expect("Dags(dags=(...))")
        .iter()
        .filter_map(Literal::as_call)
        .find(|case| matches!(case.field("seed"), Some(Literal::Int(seed)) if seed.as_i64() == Some(SEED)))
        .and_then(|case| case.field("objects"))
        .and_then(Literal::as_tuple)
        .expect("the case's objects")
        .iter()
        .filter_map(Literal::as_call)
        .map(|object| {
            (
                text(object.field("name").expect("a name")),
                text(object.field("literal").expect("a literal")),
            )
        })
        .collect()
}

fn answers() -> String {
    let objects = objects();
    let sent = json!(objects
        .iter()
        .map(|(hash, text)| json!({ "hash": hash, "text": text }))
        .collect::<Vec<_>>())
    .to_string();
    let mut out = String::new();
    let mut put = |call: String, answer: String| writeln!(out, "{call}\n{answer}").expect("writes");

    put("printable".into(), crate::printable());
    put("verify_objects".into(), ok(crate::verify_objects(&sent)));
    let folded = ok(crate::fold(&sent, None, UNTRUSTED));
    put("fold".into(), folded.clone());
    put(
        format!("fold at {}", NOWS[1]),
        ok(crate::fold(&sent, Some(NOWS[1].into()), UNTRUSTED)),
    );
    put(
        "registers".into(),
        ok(crate::registers(&sent, None, UNTRUSTED)),
    );
    put("entries".into(), ok(crate::entries(&sent, None, UNTRUSTED)));

    let mut events = Vec::new();
    for kind in ["Created", "Completed", "Cancelled", "Reopened", "Tended"] {
        let event = ok(crate::lifecycle(kind, "alpha", NOWS[2], "bassel", "why"));
        put(format!("lifecycle {kind}"), event.clone());
        events.push(event);
    }
    let folded: Value = serde_json::from_str(&folded).expect("fold answers JSON");
    let tips: Vec<String> = serde_json::from_str::<Value>(&ok(crate::verify_objects(&sent)))
        .expect("verify answers JSON")["tips"]
        .as_array()
        .expect("tips")
        .iter()
        .map(|tip| tip.as_str().expect("a name").to_owned())
        .collect();
    let first = json!([tips[0]]).to_string();
    let all = json!(tips).to_string();
    put(
        "seal genesis".into(),
        ok(crate::seal("[]", Some(events[0].clone()))),
    );
    put(
        "seal".into(),
        ok(crate::seal(&first, Some(events[1].clone()))),
    );
    put(
        "seal onto every tip".into(),
        ok(crate::seal(&all, Some(events[4].clone()))),
    );
    put("merge_object".into(), ok(crate::merge_object(&all, None)));
    put(
        "merge_object with an event".into(),
        ok(crate::merge_object(&all, Some(events[3].clone()))),
    );

    let functions = folded["functions"].as_object().expect("functions");
    let specs = Value::Object(functions.clone()).to_string();
    let mut env = folded["env"].clone();
    env["tended"] = json!({ "gamma": ["2026-09-12T08:00:00", "2026-10-03T08:00:00"] });
    let env = env.to_string();
    let history = folded["history"].to_string();
    let composite: Value = serde_json::from_str(COMPOSITE).expect("JSON");
    let least: Value = serde_json::from_str(LEAST).expect("JSON");
    for (todo, function) in functions.iter().chain([
        (&"composite".to_owned(), &composite),
        (&"least".to_owned(), &least),
    ]) {
        let linked = ok(crate::link(&function.to_string(), &specs));
        put(format!("link {todo}"), linked.clone());
        let compiled = ok(crate::compile(&linked, &env));
        put(format!("compile {todo}"), compiled.clone());
        let printed: Value = serde_json::from_str(&compiled).expect("compile answers JSON");
        put(
            format!("term_json {todo}"),
            ok(crate::term_json(printed["term"].as_str().expect("a print"))),
        );
        for now in NOWS {
            put(
                format!("fulfillment {todo} {now}"),
                format!("{:?}", ok(crate::fulfillment(&linked, now, &env))),
            );
            put(
                format!("explain {todo} {now}"),
                ok(crate::explain(&linked, now, &env)),
            );
        }
        put(
            format!("series_knots {todo}"),
            ok(crate::series_knots(
                &linked,
                "2026-08-01T00:00:00",
                "2026-11-01T00:00:00",
                &history,
            )),
        );
    }
    out
}

#[test]
fn every_export_answers_the_snapshot() {
    let answers = answers();
    if std::env::var_os("PRODROME_BLESS").is_some() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/snapshots/dag-1.txt");
        std::fs::write(path, &answers).expect("writes the snapshot");
        return;
    }
    for (line, (mine, pinned)) in answers.lines().zip(SNAPSHOT.lines()).enumerate() {
        assert_eq!(mine, pinned, "snapshot line {}", line + 1);
    }
    assert_eq!(answers, SNAPSHOT);
}

/// The todo schema through the generic path, `Todos`, answers what the
/// reference module's own exports do over the same DAG: the same findings,
/// every row's price, provisionality, function and stream in the same list
/// order, and the same functions to link against.
#[test]
fn the_todo_class_answers_as_the_reference_exports() {
    let sent = json!(objects()
        .iter()
        .map(|(hash, text)| json!({ "hash": hash, "text": text }))
        .collect::<Vec<_>>())
    .to_string();
    let todos = ok(crate::Todos::new(&sent));
    let parse = |text: String| -> Value { serde_json::from_str(&text).expect("JSON") };

    assert_eq!(ok(todos.verify()), ok(crate::verify_objects(&sent)));

    let mine = parse(ok(todos.entries(None, UNTRUSTED)));
    let theirs = parse(ok(crate::entries(&sent, None, UNTRUSTED)));
    assert_eq!(mine["at"], theirs["at"]);
    let rows = mine["entries"].as_array().expect("rows");
    let order: Vec<&Value> = rows.iter().map(|row| &row["todo"]).collect();
    let pinned: Vec<&Value> = theirs["order"].as_array().expect("order").iter().collect();
    assert_eq!(order, pinned);
    for row in rows {
        let todo = &row["todo"];
        let theirs = theirs["entries"]
            .as_array()
            .expect("rows")
            .iter()
            .find(|entry| entry["todo"] == *todo)
            .expect("the same todo");
        for key in [
            "value",
            "unlinked",
            "unconfirmed",
            "spec",
            "stream",
            "conflicts",
        ] {
            assert_eq!(row[key], theirs[key], "{todo} {key}");
        }
    }

    let prices = parse(ok(todos.prices(None, UNTRUSTED)));
    let folded = parse(ok(crate::fold(&sent, None, UNTRUSTED)));
    let prices = prices["prices"].as_array().expect("per prodrome");
    assert_eq!(prices.len(), 1, "seed 1 is one prodrome");
    assert_eq!(prices[0]["functions"], folded["functions"]);
    assert_eq!(prices[0]["env"], folded["env"]);

    let readings = parse(ok(todos.readings(None, UNTRUSTED)));
    assert!(!readings["readings"].as_array().expect("rows").is_empty());
}

/// `Todos::next_changes` answers, per row, what [`crate::next_change`]
/// answers of that row's function linked against its prodrome's, under the
/// environment `prices` gives: one reading of the price, at every instant.
#[test]
fn the_todo_class_answers_each_row_s_next_change() {
    let sent = json!(objects()
        .iter()
        .map(|(hash, text)| json!({ "hash": hash, "text": text }))
        .collect::<Vec<_>>())
    .to_string();
    let todos = ok(crate::Todos::new(&sent));
    let parse = |text: String| -> Value { serde_json::from_str(&text).expect("JSON") };
    let percent = r#"{"levels": 100, "rounding": "nearest"}"#;
    for now in NOWS {
        let changes = parse(ok(todos.next_changes(Some(now.into()), UNTRUSTED, percent)));
        let prices = parse(ok(todos.prices(Some(now.into()), UNTRUSTED)));
        let (changes, prices) = (&changes["changes"][0], &prices["prices"][0]);
        let functions = prices["functions"].as_object().expect("functions");
        let specs = Value::Object(functions.clone()).to_string();
        let env = prices["env"].to_string();
        let next = changes["next"].as_object().expect("per row");
        assert_eq!(next.len(), functions.len(), "a row per function at {now}");
        for (todo, function) in functions {
            let linked = ok(crate::link(&function.to_string(), &specs));
            let theirs = parse(ok(crate::next_change(&linked, now, &env, percent)));
            assert_eq!(next[todo], theirs, "{todo} at {now}");
        }
    }
}
