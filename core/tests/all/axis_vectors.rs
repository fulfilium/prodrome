//! Law 92 over `conformance/axis.py`: terms whose spans move an instant past
//! an end of the time axis (SPEC §7), and the readings that the rule gives.
//! The values come from the rule and not from the reference, which stops on
//! these terms. Each reading is checked on the term and on its normal form.

use crate::common;

use common::vectors::{each, field, moment, text_at, vectors};
use prodrome::fpl::{fulfillment, instant_of, parse_term, Closed, Env};

/// Law 92: a shift, a window, a cycle and a schedule that reach past an end of
/// the axis read as the rule says, and the normal form reads alike.
#[test]
fn every_reading_past_an_end_of_the_axis_is_the_rules() {
    let data = vectors("axis.py");
    let cases = each(&data, "cases");
    assert_eq!(cases.len(), 7, "axis.py lost cases");
    for case in cases {
        let why = text_at(case, "why");
        let term = parse_term(text_at(case, "term"))
            .unwrap_or_else(|e| panic!("{why}: cannot parse: {e}"));
        let closed = Closed::of(term).expect("the vectors hold no Ref");
        let normal = closed.normalize();
        for sample in each(case, "samples") {
            let now = instant_of(moment(field(sample, "now")));
            for (form, term) in [("term", &closed), ("normal form", &normal)] {
                common::reading(
                    why,
                    fulfillment(term, now, &Env::new()),
                    field(sample, "value"),
                )
                .unwrap_or_else(|e| panic!("{why}, {form} at {now}: {e}"));
            }
        }
    }
}
