# Issues on the roadmap: the mirror and the pricing bot

This repository keeps its roadmap as a Prodrome store on the `roadmap-data`
branch. Two workflows keep GitHub issues and that store in step:

- **`mirror.yml`** turns what happens to an issue into events in the store.
- **`price.yml`** asks Claude to suggest a price for a new or edited issue,
  and records the suggestion as a *claim*: something the store keeps and
  shows, but does not believe until a maintainer says so.

Both run one command, `prodrome-github`, which does all the deciding. The
workflows only fetch, run it, and push.

## The flow

```
issue opened ──► mirror.yml ──► prodrome-github ──► roadmap-data: gh-N at 80%
     │                                               + one welcome comment
     │
     └────────► price.yml
                  propose (Claude, read-only, no tools)
                     │ reply.md
                  record (no model) ──► prodrome-github --proposal
                                        ──► roadmap-data: pricing-bot's claim
                                        + the reply, posted as a comment

"/price 40" by a maintainer ──► mirror.yml ──► the maintainer's own price
"/price accept"             ──► mirror.yml ──► the bot's claim, as the
                                               maintainer's own price
```

### What each GitHub event becomes

The item for issue `#N` is `gh-N`, and its page in the viewer is
`https://fulfilium.github.io/prodrome/#/todo/gh-N`.

| GitHub event | What is appended | Written as |
|---|---|---|
| issue opened | `Created`, and a record: the title as its body, the issue's URL as `source.message_id`, category `issue`, a flat 80% | `github-mirror` |
| title edited | a new record with the new title and no spec, so the price in force stays | `github-mirror` |
| body edited, labelled, assigned… | nothing | |
| issue closed | `Completed` | `github-mirror` |
| issue reopened | `Reopened` | `github-mirror` |
| a maintainer's `/price` comment | `SpecRevised` | the commenter's login, lowercased |
| a pricing reply | `SpecRevised` | `pricing-bot` |
| anything on a pull request | nothing | |

An issue opened before the mirror existed is created by the first event that
mentions it.

### The `/price` grammar

Only the first line of a comment is read, and only when it starts with
`/price`:

- `/price 40`: a flat fulfillment of 40%. Low is urgent.
- `/price 30 --deadline 2026-10-15`: 30% when the lead-up begins, decaying to
  5% at 17:00 on that day; `--end N` and `--lead-up DAYS` change the two
  defaults (5% and 3 days), the same ones `prodrome revise --deadline` uses.
- `/price ref gh-7`: exactly as urgent as item `gh-7`, which must already be
  in the store.
- `/price accept`: take the pricing bot's latest suggestion for this issue
  (the latest one dated no later than the comment) as the commenter's own.

The scale — what 10%, 30%, 50%, 70% and 90% mean — is in `PRICING.md` on the
`roadmap-data` branch.

## Trust boundaries

**Who may price.** GitHub tells the workflow how a commenter is related to
the repository (`author_association`). Only `OWNER`, `MEMBER` and
`COLLABORATOR` may price. This is checked *before* the `/price` line is
parsed, so a stranger's malformed `/price` is silently ignored rather than
answered: nobody outside the project can make the workflow speak. A
maintainer's malformed `/price` gets a reply saying what was wrong. No login
can sign as `github-mirror` or `pricing-bot`.

**What the bot's word is worth.** The store holds no policy of its own
(SPEC §5): a *reader* decides whose events bind. The bot's suggestions are
written as `pricing-bot` so that a reader can run

```console
$ prodrome list --untrusted pricing-bot
```

and see the confirmed price beside the claimed one. Under that reading a
suggestion changes nothing until a maintainer's `/price accept` re-issues it
under their own name. A reader who names nobody untrusted believes every
writer, the bot included, so a reader who wants only what maintainers
decided should always name it.

A claim is dated no earlier than the latest event already in the store,
because the model takes minutes and other events land meanwhile. SPEC §3's
`verify` refuses an untrusted event dated behind what it was written on, and
this keeps `prodrome verify --untrusted pricing-bot` clean.

**What the model can do.** The issue's title and body are written by anyone,
so they are treated as hostile input:

- The model runs in its own job (`propose`) whose token can only *read*
  contents and issues.
- It has no tools (`--tools ""`), no MCP servers (`--strict-mcp-config`, and
  the action adds none in this mode) and one turn (`--max-turns 1`). It can
  read the text it is given and reply, and nothing else.
- The issue's text reaches it only inside the prompt, fenced and labelled as
  data. The text is read out of the event file with `jq`, never pasted into a
  shell command.
- Its reply leaves the job as a file. A second job (`record`), which has
  write permissions and no model, parses the reply's `/price` line with the
  same grammar a maintainer's comment goes through, refuses `accept`, and
  appends it as `pricing-bot`. The worst a manipulated reply can do is
  propose a wrong price, which binds nothing, or post an odd comment; every
  `@` in the posted reply is broken so it cannot mention anyone.
- The action is pinned to a commit, and it authenticates with the
  `CLAUDE_CODE_OAUTH_TOKEN` secret, which only the `propose` job sees.

**What code runs.** Both workflows trigger only on `issues` and
`issue_comment`, which GitHub runs from the default branch. The only code
they check out and build is `main`. There is no `pull_request_target`
trigger, and nothing from a pull request is ever checked out.

**What gets pushed.** Only `roadmap-data`, only after `prodrome verify`
passes, and only new files under `roadmap/objects/`: an object is named by the
hash of its bytes and never rewritten. Two runs racing to push append
disjoint sets of files, so the loser merges and pushes again; the union is
the store, and nothing has to join it.

## Determinism

`prodrome-github` reads no clock and no network. Every instant it writes
comes from the payload (`created_at`, `updated_at`, `closed_at`, the comment's
`created_at`), so the same delivery always means the same events, and an
event the store already holds is not appended again. Re-running a workflow,
or GitHub delivering an event twice, adds nothing.

Two refusals depend on the store — `/price ref` to an item not yet mirrored,
and `/price accept` before any suggestion exists — and a later replay of such
a delivery applies once what it names exists. The laws are stated over the
deliveries that were applied, and pinned as property tests in
`github/tests/github.rs`:

- replaying what was applied appends nothing, and a fresh store given those
  deliveries twice is identical, file for file, to one given them once;
- close then reopen folds to open.

## Known limits

- A closed issue becomes `Completed`, whatever GitHub's `state_reason` says;
  "not planned" is not yet `Cancelled`.
- The store folds events in the order they were appended, and GitHub does not
  promise to deliver webhooks in order. A close and a reopen seconds apart can
  in principle land out of order; the next close or reopen settles it.
- The workflows do not use a `concurrency` group, because GitHub keeps only
  one pending run per group and would drop the rest; the push retries instead.
