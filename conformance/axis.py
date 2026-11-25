# conformance/axis.py — SPEC §7, "The time axis", and law 92. Each case is a
# term whose spans move an instant past an end of the time axis, and the
# readings that the rule gives: the instant stops at the end that it passes.
# The values were derived by hand from the rule and the §7 semantics, and not
# taken from the reference, which stops on these terms. Frozen evidence:
# nothing under `cargo test` writes this file. One case per line.
Axis(cases=(
AxisCase(why='a shift past the last instant reads the body at the last instant', term='Shift(delta=timedelta(days=100000000), term=Decay(start=0.5, end=0.1, end_date=datetime(2026, 9, 15, 9, 0, 0), lead_up=timedelta(days=3), start_date=None))', samples=(Sample(now=datetime(2026, 9, 14, 12, 0, 0), value=0.1), Sample(now=datetime(9999, 12, 31, 23, 59, 59, 999999), value=0.1))),
AxisCase(why='a shift before the first instant reads the body at the first instant', term='Shift(delta=timedelta(days=-100000000), term=Decay(start=0.5, end=0.1, end_date=datetime(2026, 9, 15, 9, 0, 0), lead_up=timedelta(days=3), start_date=None))', samples=(Sample(now=datetime(2026, 9, 14, 12, 0, 0), value=0.98), Sample(now=datetime(1, 1, 1, 0, 0, 0), value=0.98))),
AxisCase(why='two shifts that cancel on a line do not cancel where the first stops at an end', term='Shift(delta=timedelta(days=100000000), term=Shift(delta=timedelta(days=-100000000), term=Decay(start=0.5, end=0.1, end_date=datetime(2026, 9, 15, 9, 0, 0), lead_up=timedelta(days=3), start_date=None)))', samples=(Sample(now=datetime(2026, 9, 14, 12, 0, 0), value=0.98),)),
AxisCase(why='the samples of a window that passes the last instant stop at it', term='Within(window=timedelta(days=100000000), p=-1.0, term=Decay(start=0.5, end=0.1, end_date=datetime(2026, 9, 15, 9, 0, 0), lead_up=timedelta(days=3), start_date=None))', samples=(Sample(now=datetime(2026, 9, 14, 12, 0, 0), value=0.10083532219570406),)),
AxisCase(why='a cycle that ends after the last instant folds onto the last instant', term='Periodic(period=timedelta(days=100000000), anchor=datetime(2026, 9, 10, 9, 0, 0), term=Decay(start=0.5, end=0.1, end_date=datetime(2026, 9, 15, 9, 0, 0), lead_up=timedelta(days=3), start_date=None))', samples=(Sample(now=datetime(2026, 9, 1, 0, 0, 0), value=0.1),)),
AxisCase(why='a shift before the first instant reads a schedule at its head', term='Shift(delta=timedelta(days=-100000000), term=Piecewise(head=Flat(value=0.3), pieces=(Piece(at=datetime(2026, 9, 10, 9, 0, 0), term=Flat(value=0.7)),)))', samples=(Sample(now=datetime(2026, 9, 14, 12, 0, 0), value=0.3),)),
AxisCase(why='a shift past the last instant reads a schedule at its last piece', term='Shift(delta=timedelta(days=100000000), term=Piecewise(head=Flat(value=0.3), pieces=(Piece(at=datetime(2026, 9, 10, 9, 0, 0), term=Flat(value=0.7)),)))', samples=(Sample(now=datetime(2026, 9, 14, 12, 0, 0), value=0.7),)),
))
