//! The verbs, driven end to end against a store on disk.
//!
//! Through `Cli::try_parse_from` and never through a spawned process: the
//! command line's grammar is part of what is being tested, and the answer is a
//! value the library returned rather than bytes scraped off a pipe.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use prodrome::event::{canonical_envelope, mk_created, mk_sealed, seal_hash};
use prodrome::literal::Datetime;
use prodrome::policy::Untrusted;
use prodrome::store::Sender;
use prodrome_cli::command::Cli;
use prodrome_cli::{run, Outcome, Store};

use clap::Parser;

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn a_store() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "prodrome-cli-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

/// One invocation. `--store` is always named, because a test that fell back to
/// `./roadmap` would be reading the repository's own store.
fn prodrome(root: &Path, args: &[&str]) -> Result<Outcome, prodrome_cli::Error> {
    let mut argv: Vec<String> = vec!["prodrome".to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    argv.push("--store".to_owned());
    argv.push(root.display().to_string());
    run(&Cli::try_parse_from(argv).expect("the arguments parse"))
}

fn said(root: &Path, args: &[&str]) -> String {
    prodrome(root, args).expect("the verb answers").text
}

/// A seeded store: two todos, one of them completed, all of it backdated so
/// the time-machine assertions below have a past to ask about.
fn seeded() -> PathBuf {
    let root = a_store();
    said(&root, &["init", &root.display().to_string()]);
    said(
        &root,
        &[
            "add",
            "publish",
            "--body",
            "publish the crate",
            "--priority",
            "70",
            "--actor",
            "bassel",
            "--at",
            "2026-09-08T09:00:00",
        ],
    );
    said(
        &root,
        &[
            "add",
            "ship-the-cli",
            "--body",
            "a command line over a store",
            "--detail",
            "the verbs, and a roadmap seeded with them",
            "--priority",
            "35",
            "--actor",
            "bassel",
            "--at",
            "2026-09-09T09:00:00",
        ],
    );
    said(
        &root,
        &[
            "done",
            "ship-the-cli",
            "--actor",
            "bassel",
            "--at",
            "2026-09-10T09:00:00",
        ],
    );
    root
}

#[test]
fn init_makes_a_store_once() {
    let root = a_store();
    let dir = root.display().to_string();
    assert!(said(&root, &["init", &dir]).contains("initialised"));
    assert!(root.join("objects").is_dir());
    assert!(
        prodrome(&root, &["init", &dir]).is_err(),
        "twice is a refusal"
    );
}

#[test]
fn list_is_ascending_in_fulfillment() {
    let root = seeded();
    let text = said(&root, &["list", "--at", "2026-09-09T12:00:00"]);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].starts_with("2 open at 2026-09-09T12:00:00"),
        "{text}"
    );
    assert!(lines[1].starts_with("0.350  ship-the-cli"), "{text}");
    assert!(lines[2].starts_with("0.700  publish"), "{text}");
}

#[test]
fn a_todo_is_absent_before_it_was_created() {
    let root = seeded();
    let text = said(&root, &["list", "--at", "2026-09-08T12:00:00"]);
    assert!(text.starts_with("1 open"), "{text}");
    assert!(text.contains("publish"), "{text}");
    assert!(!text.contains("ship-the-cli"), "{text}");
}

#[test]
fn a_completed_todo_leaves_the_list() {
    let root = seeded();
    let text = said(&root, &["list", "--at", "2026-09-10T12:00:00"]);
    assert!(text.starts_with("1 open"), "{text}");
    assert!(!text.contains("ship-the-cli"), "{text}");
}

#[test]
fn reopen_brings_it_back() {
    let root = seeded();
    said(
        &root,
        &[
            "reopen",
            "ship-the-cli",
            "--actor",
            "bassel",
            "--at",
            "2026-09-10T10:00:00",
        ],
    );
    let text = said(&root, &["list", "--at", "2026-09-10T12:00:00"]);
    assert!(text.starts_with("2 open"), "{text}");
}

#[test]
fn cancel_closes_a_todo_too() {
    let root = seeded();
    said(
        &root,
        &[
            "cancel",
            "publish",
            "--actor",
            "bassel",
            "--at",
            "2026-09-10T10:00:00",
        ],
    );
    let text = said(&root, &["show", "publish", "--at", "2026-09-10T12:00:00"]);
    assert!(text.contains("state      cancelled"), "{text}");
}

#[test]
fn revise_reprices_from_its_instant_on() {
    let root = seeded();
    said(
        &root,
        &[
            "revise",
            "publish",
            "--priority",
            "20",
            "--actor",
            "bassel",
            "--at",
            "2026-09-10T09:00:00",
        ],
    );
    let before = said(&root, &["list", "--at", "2026-09-10T08:00:00"]);
    let after = said(&root, &["list", "--at", "2026-09-10T12:00:00"]);
    assert!(before.contains("0.700  publish"), "{before}");
    assert!(after.contains("0.200  publish"), "{after}");
}

#[test]
fn revise_without_a_price_is_a_refusal() {
    let root = seeded();
    assert!(prodrome(&root, &["revise", "publish", "--actor", "bassel"]).is_err());
}

#[test]
fn an_unpriced_todo_reads_absent_and_lists_after_every_priced_one() {
    let root = seeded();
    said(
        &root,
        &[
            "add",
            "a-note",
            "--body",
            "no claim on attention",
            "--actor",
            "bassel",
            "--at",
            "2026-09-08T08:00:00",
        ],
    );
    let list = said(&root, &["list", "--at", "2026-09-09T12:00:00"]);
    let rows: Vec<&str> = list.lines().skip(1).collect();
    assert_eq!(rows.len(), 3, "{list}");
    assert!(rows[0].starts_with("0.350  ship-the-cli"), "{list}");
    assert!(rows[1].starts_with("0.700  publish"), "{list}");
    assert!(rows[2].starts_with("  ∅    a-note"), "{list}");
    let show = said(&root, &["show", "a-note", "--at", "2026-09-09T12:00:00"]);
    assert!(show.contains("price      absent"), "{show}");
    assert!(show.contains("spec       Absent()"), "{show}");
}

#[test]
fn a_verb_refuses_an_id_the_store_does_not_hold() {
    let root = seeded();
    assert!(prodrome(&root, &["done", "nope", "--actor", "bassel"]).is_err());
    assert!(prodrome(&root, &["show", "nope"]).is_err());
    assert!(prodrome(
        &root,
        &["add", "publish", "--body", "again", "--actor", "bassel"]
    )
    .is_err());
}

#[test]
fn show_carries_the_record_and_the_stream() {
    let root = seeded();
    let text = said(
        &root,
        &["show", "ship-the-cli", "--at", "2026-09-10T12:00:00"],
    );
    assert!(text.contains("state      completed"), "{text}");
    assert!(text.contains("created    2026-09-09T09:00:00"), "{text}");
    assert!(text.contains("detail     the verbs"), "{text}");
    assert!(text.contains("confidence confirmed"), "{text}");
    // Created, SpecRevised, the record, and the completion.
    assert_eq!(text.matches("stream").count(), 1, "{text}");
}

#[test]
fn an_untrusted_writer_claims_and_the_row_says_so() {
    let root = seeded();
    let text = said(
        &root,
        &[
            "list",
            "--at",
            "2026-09-10T12:00:00",
            "--untrusted",
            "bassel",
        ],
    );
    assert!(text.contains("untrusted: bassel"), "{text}");
    // The completion only CLAIMS, so the confirmed reading still has it open,
    // and the row carries the claim rather than hiding it.
    assert!(text.contains("ship-the-cli"), "{text}");
    assert!(
        text.contains("[claimed completed, content unconfirmed]"),
        "{text}"
    );
}

#[test]
fn verify_passes_a_store_the_verbs_wrote() {
    let root = seeded();
    let outcome = prodrome(&root, &["verify"]).expect("verify answers");
    assert!(outcome.ok, "{}", outcome.text);
    assert!(outcome.text.starts_with("ok: "), "{}", outcome.text);
}

#[test]
fn verify_fails_a_tampered_object() {
    let root = seeded();
    let objects = std::fs::read_dir(root.join("objects")).expect("the store has objects");
    let victim = objects
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .next()
        .expect("at least one object");
    std::fs::write(&victim, "Sealed(prev=None, event=None)").expect("tampers");
    let outcome = prodrome(&root, &["verify"]).expect("verify answers with findings");
    assert!(!outcome.ok, "{}", outcome.text);
    assert!(outcome.text.contains("does not hash"), "{}", outcome.text);
}

/// A damaged tip stops every write until `fsck` sets it aside; then the store
/// writes again, and `verify` fails on the receipt alone. A healthy store's
/// `fsck` moves nothing and passes.
#[test]
fn fsck_sets_a_damaged_object_aside_and_the_store_writes_again() {
    let root = seeded();
    let healthy = prodrome(&root, &["fsck"]).expect("fsck answers");
    assert!(healthy.ok, "{}", healthy.text);
    let store = Store::new(&root, Untrusted::none());
    // A tip that is neither the genesis nor what names a todo, so the store
    // still knows `publish` once it is set aside.
    let tip = store
        .tips()
        .expect("the tips derive")
        .into_iter()
        .find(|tip| {
            store
                .load(tip)
                .expect("loads")
                .event()
                .is_some_and(|event| event.kind_name() != "Created")
        })
        .expect("a tip");
    let victim = root.join("objects").join(format!("{}.py", tip.as_str()));
    std::fs::write(&victim, "Sealed(prev=None, event=None)").expect("damages");

    let done = ["done", "publish", "--actor", "bassel"];
    let refusal = prodrome(&root, &done).expect_err("no honest tips");
    assert!(refusal.to_string().contains("prodrome fsck"), "{refusal}");

    let outcome = prodrome(&root, &["fsck"]).expect("fsck answers");
    assert!(!outcome.ok, "the report names what it set aside");
    assert_eq!(outcome.text.lines().count(), 1, "{}", outcome.text);
    assert!(
        outcome
            .text
            .starts_with(&format!("quarantine/{}.py", tip.as_str())),
        "{}",
        outcome.text
    );
    assert!(!victim.exists());
    let outcome = prodrome(&root, &["verify"]).expect("verify answers");
    assert!(!outcome.ok);
    assert_eq!(outcome.text.lines().count(), 1, "{}", outcome.text);
    assert!(outcome.text.starts_with("quarantine/"), "{}", outcome.text);
    assert_eq!(said(&root, &done).len(), 64, "the store writes again");
}

/// A snapshot names the store as it stands: asked again with nothing new it
/// is the same one, and after a write it is a new one chained to it.
#[test]
fn snapshot_attests_the_store_and_chains() {
    let root = seeded();
    let first = said(&root, &["snapshot"]);
    assert_eq!(first.len(), 64, "{first}");
    assert_eq!(
        said(&root, &["snapshot"]),
        first,
        "nothing new, nothing written"
    );
    said(
        &root,
        &[
            "reopen",
            "ship-the-cli",
            "--actor",
            "bassel",
            "--at",
            "2026-09-11T09:00:00",
        ],
    );
    let second = said(&root, &["snapshot"]);
    assert_ne!(second, first);
    let print = std::fs::read_to_string(root.join("objects").join(format!("{second}.py")))
        .expect("the snapshot");
    assert!(print.contains(&format!("previous='{first}'")), "{print}");
    let outcome = prodrome(&root, &["verify"]).expect("verify answers");
    assert!(outcome.ok, "{}", outcome.text);
}

/// Weaving is a legacy store's: a store of changes has nothing to weave, and
/// a legacy store's two heads are settled into one.
#[test]
fn weave_settles_a_legacy_store_s_two_heads_and_is_a_no_op_on_changes() {
    assert!(said(&seeded(), &["weave"]).contains("nothing to weave"));

    // Two histories on one root, the way two replicas wrote before changes.
    let root = a_store();
    let store = Store::new(&root, Untrusted::none());
    let at = |hour| Datetime::new(2026, 9, 10, hour, 0, 0, 0).expect("an instant");
    let root_object: prodrome::event::Envelope<
        prodrome::event::TodoEvent<prodrome::reference::Todo>,
    > = mk_sealed(
        None,
        mk_created("publish", at(9), "bassel", "publish the crate", "").expect("an event"),
    );
    let elder = seal_hash(&root_object);
    let branches = ["a-first-branch", "a-second-branch"].map(|todo| {
        mk_sealed(
            Some(elder.clone()),
            mk_created(todo, at(11), "bassel", "written elsewhere", "").expect("an event"),
        )
    });
    for object in [&root_object].into_iter().chain(&branches) {
        let name = seal_hash(object);
        store
            .adopt_objects(
                &[(name.clone(), canonical_envelope(object))].into(),
                &name,
                Sender::Stranger,
            )
            .expect("writes");
    }
    assert_eq!(
        store.tips().expect("the tips derive").len(),
        2,
        "the store has forked"
    );

    let merge = said(&root, &["weave"]);
    assert_eq!(merge.len(), 64, "a weave answers with the object it wrote");
    assert_eq!(
        store.tips().expect("the tips derive").len(),
        1,
        "and the store is one history again"
    );
    assert!(store.verify().is_empty(), "{:?}", store.verify());
    assert!(said(&root, &["weave"]).contains("nothing to weave"));

    let text = said(&root, &["list", "--at", "2026-09-10T12:00:00"]);
    assert!(text.contains("a-second-branch"), "{text}");
}

/// A READING OMITS, AND SAYS SO. The object completing a todo is damaged on
/// disk: `list` and `show` still answer, reading the todo as open since what
/// closed it is left out, and each says what it left out, by name and why,
/// for the standard error; a write refuses, naming the file.
#[test]
fn a_reading_says_what_it_left_out() {
    let root = seeded();
    let healthy = prodrome(&root, &["list", "--at", "2026-09-10T12:00:00"]).expect("answers");
    assert!(healthy.left_out.is_empty(), "{:?}", healthy.left_out);
    let completed = std::fs::read_dir(root.join("objects"))
        .expect("lists")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| std::fs::read_to_string(path).is_ok_and(|text| text.contains("Completed(")))
        .expect("the completion is a file");
    let name = completed
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("a name")
        .to_owned();
    let mut bytes = std::fs::read(&completed).expect("reads");
    bytes.push(b' ');
    std::fs::write(&completed, bytes).expect("damages");

    let listed = prodrome(&root, &["list", "--at", "2026-09-10T12:00:00"]).expect("answers");
    assert!(listed.text.contains("ship-the-cli"), "{}", listed.text);
    let shown = prodrome(&root, &["show", "ship-the-cli"]).expect("answers");
    for outcome in [&listed, &shown] {
        assert_eq!(outcome.left_out.len(), 1, "{:?}", outcome.left_out);
        let line = &outcome.left_out[0];
        assert!(line.contains(&name), "{line}");
        assert!(line.contains("does not hash to its filename"), "{line}");
        assert!(line.contains("0 objects resting on it"), "{line}");
    }
    let refusal = prodrome(&root, &["done", "publish", "--actor", "bassel"])
        .expect_err("a write does not guess")
        .to_string();
    assert!(refusal.contains(&name), "{refusal}");
}
