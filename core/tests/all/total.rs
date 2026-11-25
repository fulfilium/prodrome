//! Law 92: the evaluator is total over every term that the smart constructors
//! build. The spans and instants of the terms in this file come from the
//! whole range that the constructors admit, with the ends weighted in: spans
//! of more than 262,000 years, and instants at the ends of the grammar's
//! years and of the time axis.

use std::collections::BTreeSet;

use chrono::{Duration, NaiveDate};
use prodrome::breaks::series_knots;
use prodrome::event::{mk_created, mk_spec_revised};
use prodrome::fpl::{
    self, advance, explained, mk_after, mk_decay, mk_flat, mk_gate, mk_importance, mk_least,
    mk_offset, mk_periodic, mk_piecewise, mk_recur, mk_shift, mk_within, parse_term, print_term,
    Closed, Env, Instant, Outcome, FIRST, LAST,
};
use prodrome::observe::{next_change, Observation};
use prodrome::policy::Untrusted;
use prodrome::term::{CurvePoint, Term};
use proptest::prelude::*;

use crate::common::terms::{moment, ok, EVENTS};
use crate::common::{at, flatten};

fn calendar(year: i32, month: u32, day: u32, micro: u32) -> Instant {
    NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|date| date.and_hms_micro_opt(23, 59, 59, micro))
        .expect("a real instant")
}

/// An instant that a term may hold: the ends of the grammar's years, and
/// the hourly grid around the origin.
fn an_instant() -> impl Strategy<Value = Instant> {
    prop_oneof![
        3 => Just(calendar(1, 1, 1, 0).date().and_hms_opt(0, 0, 0).expect("midnight")),
        3 => Just(calendar(9999, 12, 31, 999_999)),
        6 => (-400i64..400).prop_map(moment),
    ]
}

/// An instant that a term is read at: the ends of the axis as well.
fn a_reading() -> impl Strategy<Value = Instant> {
    prop_oneof![
        2 => Just(FIRST),
        2 => Just(LAST),
        1 => Just(advance(LAST, -Duration::microseconds(1))),
        4 => an_instant(),
    ]
}

/// A span that a term may hold, from one microsecond to the longest span
/// that a constructor admits, in either direction.
fn a_span() -> impl Strategy<Value = Duration> {
    prop_oneof![
        3 => Just(Duration::microseconds(i64::MAX)),
        3 => Just(Duration::microseconds(i64::MIN + 1)),
        1 => Just(Duration::microseconds(i64::MIN)),
        2 => (i64::MAX - 1_000..=i64::MAX).prop_map(Duration::microseconds),
        2 => (-95_800_000i64..=95_800_000).prop_map(Duration::days),
        2 => any::<i64>().prop_map(Duration::microseconds),
        3 => (-400i64..400).prop_map(Duration::hours),
        1 => Just(Duration::microseconds(1)),
        1 => Just(Duration::zero()),
    ]
}

fn a_positive_span() -> impl Strategy<Value = Duration> {
    a_span().prop_map(|span| {
        span.abs()
            .clamp(Duration::microseconds(1), Duration::microseconds(i64::MAX))
    })
}

fn a_leaf() -> BoxedStrategy<Term> {
    prop_oneof![
        3 => (0.02f64..0.98).prop_map(|v| ok(mk_flat(v))),
        4 => (
            0.3f64..0.95,
            0.0f64..0.2,
            an_instant(),
            a_positive_span(),
            prop::option::of(an_instant()),
        )
            .prop_map(|(start, end, end_date, lead, from)| ok(mk_decay(
                start, end, end_date, lead, from
            ))),
        2 => (
            prop::collection::btree_set(an_instant(), 1..4),
            prop::collection::vec(0.0f64..1.0, 4),
        )
            .prop_map(|(ats, values)| ok(fpl::mk_curve(
                ats.into_iter()
                    .zip(values)
                    .map(|(at, value)| CurvePoint { at, value, label: String::new() })
                    .collect()
            ))),
        1 => Just(fpl::mk_absent()),
    ]
    .boxed()
}

/// Every constructor over `leaf` except `Within`, which a window nests into
/// 65 samples for each level.
fn a_wide_base() -> BoxedStrategy<Term> {
    a_leaf()
        .prop_recursive(3, 14, 3, |inner| {
            prop_oneof![
                (a_span(), inner.clone()).prop_map(|(d, t)| ok(mk_shift(d, t))),
                (-0.9f64..0.9, inner.clone()).prop_map(|(d, t)| ok(mk_offset(d, t))),
                (inner.clone(), inner.clone()).prop_map(|(g, b)| ok(mk_gate(g, b))),
                (0.3f64..2.5, inner.clone()).prop_map(|(w, t)| ok(mk_importance(w, t))),
                prop::collection::vec(inner.clone(), 1..3).prop_map(|ts| ok(mk_least(ts))),
                (a_positive_span(), an_instant(), inner.clone())
                    .prop_map(|(p, a, t)| ok(mk_periodic(p, a, t))),
                (
                    prop::sample::select(EVENTS.to_vec()),
                    an_instant(),
                    inner.clone(),
                    inner.clone()
                )
                    .prop_map(|(e, a, t, p)| ok(mk_after(
                        e.to_owned(),
                        a,
                        t,
                        p,
                        None
                    ))),
                (
                    prop::sample::select(EVENTS.to_vec()),
                    an_instant(),
                    inner.clone(),
                    inner.clone()
                )
                    .prop_map(|(e, a, t, p)| ok(mk_recur(e.to_owned(), a, t, p))),
                (
                    inner.clone(),
                    prop::collection::btree_set(an_instant(), 1..3),
                    prop::collection::vec(inner, 3)
                )
                    .prop_map(|(head, ats, ts)| ok(mk_piecewise(
                        head,
                        ats.into_iter().zip(ts).collect()
                    ))),
            ]
        })
        .boxed()
}

fn a_wide_term() -> impl Strategy<Value = Term> {
    prop_oneof![
        4 => a_wide_base(),
        1 => (a_positive_span(), prop::sample::select(vec![-4.0, -1.0, 0.0]), a_wide_base())
            .prop_map(|(w, p, t)| ok(mk_within(w, p, t))),
        1 => (a_span(), a_positive_span(), a_wide_base())
            .prop_map(|(d, w, t)| ok(mk_shift(d, ok(mk_within(w, -1.0, t))))),
    ]
}

/// Each of `EVENTS` bound or tended at instants of the whole range.
fn a_wide_env() -> impl Strategy<Value = Env> {
    let bound = prop::option::of((any::<bool>(), an_instant()));
    (
        prop::collection::vec(bound, 3),
        prop::collection::vec(prop::collection::btree_set(an_instant(), 0..3), 3),
    )
        .prop_map(|(bounds, tended)| {
            let mut env = Env::new();
            for ((name, bound), tendings) in EVENTS.iter().zip(bounds).zip(tended) {
                if let Some((completed, at)) = bound {
                    let outcome = if completed {
                        Outcome::Completed(at)
                    } else {
                        Outcome::Cancelled(at)
                    };
                    env.outcomes
                        .insert((*name).to_owned(), BTreeSet::from([Some(outcome)]));
                }
                if !tendings.is_empty() {
                    env.tended.insert((*name).to_owned(), tendings);
                }
            }
            env
        })
}

fn unit(value: Option<f64>) -> bool {
    value.is_none_or(|v| (0.0..=1.0).contains(&v))
}

proptest! {
    #![proptest_config(crate::common::cases::cases(2000))]

    /// Law 92: for every term that the constructors build, every reading
    /// instant of the axis and every environment, `fulfillment`, `explained`,
    /// `next_change`, the curve's sampling and `flatten` return, and every
    /// number they give is in the unit interval. A term prints and parses as
    /// itself, and its normal form reads as it does.
    #[test]
    fn the_evaluator_is_total_over_every_constructible_term(
        term in a_wide_term(),
        env in a_wide_env(),
        readings in prop::collection::vec(a_reading(), 3),
    ) {
        let printed = print_term(&term);
        prop_assert_eq!(parse_term(&printed).expect("a stored term parses"), term.clone());
        let closed = Closed::of(term.clone()).expect("the generator builds no reference");
        let normal = closed.normalize();
        for now in readings {
            let value = fpl::fulfillment(&closed, now, &env);
            prop_assert!(unit(value), "{value:?}");
            let normal_value = fpl::fulfillment(&normal, now, &env);
            prop_assert_eq!(value.is_some(), normal_value.is_some());
            if let (Some(a), Some(b)) = (value, normal_value) {
                prop_assert!((a - b).abs() < 1e-9, "{a} against {b}");
            }
            prop_assert!(unit(explained(&closed, now, &env).value));
            let next = next_change(&closed, now, &env, Observation::PERCENT);
            prop_assert!(next.at.is_none_or(|at| at > now), "{next:?} at {now}");
        }
    }

    /// Law 92, the curve: a series over any window of the axis holds
    /// numbers in the unit interval at instants in order.
    #[test]
    fn a_series_over_any_window_is_in_the_unit_interval(
        term in a_wide_term(),
        env in a_wide_env(),
        from in a_reading(),
        to in a_reading(),
    ) {
        let closed = Closed::of(term).expect("the generator builds no reference");
        let series = series_knots(&closed, from, to, |_| env.clone());
        for knot in &series.knots {
            prop_assert!(unit(knot.value), "{knot:?}");
        }
        prop_assert!(series.knots.windows(2).all(|pair| pair[0].at < pair[1].at));
    }

    /// Law 92, the fold: a log whose spec is any such term flattens, and each
    /// function that results prints.
    #[test]
    fn a_log_holding_any_such_spec_flattens(term in a_wide_term()) {
        let log = vec![
            crate::common::ok(mk_created("todo-1", at(1, 9, 0), "ada", "write it", "")),
            crate::common::ok(mk_spec_revised("todo-1", at(2, 9, 0), "ada", term, "")),
        ];
        let functions = flatten(&log, at(3, 9, 0), &Untrusted::of([])).expect("the log folds");
        for function in functions.values() {
            prop_assert!(parse_term(&print_term(function)).is_ok());
        }
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(512))]

    /// Law 92, the meaning: `Shift(δ, a)` read at `t` is `a` read at `t + δ`
    /// where the sum is on the axis, and at the end of the axis that the sum
    /// passes where it is not.
    #[test]
    fn a_shift_reads_its_body_at_the_sum_stopped_at_the_ends(
        body in a_wide_base(),
        env in a_wide_env(),
        delta in a_span(),
        now in a_reading(),
    ) {
        let shifted = Closed::of(ok(mk_shift(delta, body.clone()))).expect("closed");
        let body = Closed::of(body).expect("closed");
        let end = match now.checked_add_signed(delta) {
            Some(sum) if sum <= LAST => sum,
            _ if delta < Duration::zero() => FIRST,
            _ => LAST,
        };
        prop_assert_eq!(
            fpl::fulfillment(&shifted, now, &env),
            fpl::fulfillment(&body, end, &env)
        );
    }
}
