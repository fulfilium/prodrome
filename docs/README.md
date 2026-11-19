# Documentation

Start with the [guide](guide.md). [`SPEC.md`](../SPEC.md) is the
specification.

## For users

- [`guide.md`](guide.md) explains the model, how to price work, the Rust
  API, the files on disk, and WebAssembly.
- [`../SPEC.md`](../SPEC.md) is the specification. Section 9 lists the laws.
- [`../CHANGELOG.md`](../CHANGELOG.md) lists the changes in each release,
  and which changes affect the stored format.

## For contributors

- [`../CONTRIBUTING.md`](../CONTRIBUTING.md) explains the workflow for a
  change, and the rules for written English.
- [`../AGENTS.md`](../AGENTS.md) gives instructions to coding agents.
- [`development.md`](development.md) explains the tests, the CI, the
  conformance vectors, and the layout of the repository.
- [`roadmap.md`](roadmap.md) explains the roadmap branch, how issues become
  roadmap items, and how to add items.
- [`viewer.md`](viewer.md) explains the browser viewer and GitHub Pages.
- [`script-law.md`](script-law.md) explains how to run the script law
  (SPEC law 68) over an application's own schema.
- [`github-agent.md`](github-agent.md) explains the issue mirror and the
  pricing bot, and the limits of trust in them.

## Design notes

Each design note was written before its change was made. For what was
finally implemented, see `SPEC.md` and `CHANGELOG.md`.

- [`design-empty-term.md`](design-empty-term.md): a design for `Absent`, a
  term that has no value, and for the two layers of the store.
- [`design-change-identity.md`](design-change-identity.md): a design that
  names each change by its event and by the writes that it replaces.
- [`design-register-types.md`](design-register-types.md): a design for typed
  registers. Events are stored without a schema, and a program applies a
  schema when it reads the events.
- [`design-script-law.md`](design-script-law.md): a design for the script
  law, which drives three replicas through random steps and checks every
  invariant after every step.
- [`design-literal-derive.md`](design-literal-derive.md): the design of
  `#[derive(Literal)]`, the routes that were weighed, and the measurements.
- [`SIMPLIFY-PLAN.md`](SIMPLIFY-PLAN.md): a plan to remove duplicated code,
  so that the crate has one term functor, one schema, one evaluator, and one
  fold.
