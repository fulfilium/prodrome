//! A declaration admitted, and one refused by the law that it breaks.

use prodrome_declared::{Declaration, Law};

/// A door whose state is discrete is admitted; a machine whose covers go
/// round is refused under `Law::PartialOrder`.
#[test]
fn a_door_is_admitted_and_a_cycle_is_refused() {
    let text = "Schema(key='door', events=(Event(name='Moved', fields=(\
        Field(name='door', type=Text()), Field(name='at', type=Instant()), \
        Field(name='actor', type=Text()), \
        Field(name='to', type=Enum(alternatives=('open', 'shut'))))),), \
        registers=(Register(name='state', order=Discrete(), inflationary=False, \
        writes=(Write(event='Moved', field='to'),)),))";
    let door = Declaration::admit(text).expect("admitted");
    assert_eq!(door.key(), "door");

    // A machine whose covers go round is not a partial order.
    let cycle = text.replace(
        "Discrete()",
        "Machine(covers=(('open', 'shut'), ('shut', 'open')))",
    );
    assert_eq!(
        Declaration::admit(&cycle).unwrap_err().law,
        Law::PartialOrder
    );
}
