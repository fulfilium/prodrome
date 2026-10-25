# The roadmap is a Prodrome store

The roadmap of this crate is a Prodrome store. The store is on its own
branch,
[`roadmap-data`](https://github.com/fulfilium/prodrome/tree/roadmap-data),
apart from the code. Each item in the store is a todo: a piece of work that
is left for this crate. The [viewer](viewer.md) shows the roadmap in the
browser.

## Read the roadmap

Check out the branch and run `prodrome list`. The command computes the
state of every item at the time that you give, or at the current time. The
list is sorted by fulfillment from low to high, so the most urgent item is
at the top.

```console
$ git switch roadmap-data
$ prodrome list
$ prodrome list --at 2026-09-09
$ prodrome list --untrusted pricing-bot   # show the bot's suggestions as claims
```

On every pull request and every push to `main`, the CI checks out the
roadmap branch and runs `verify` and `list` on it. So a change to the code
cannot break the reading of the roadmap without the CI failing.

## Issues become items

When someone opens an issue on this repository, a workflow adds an item
`gh-<number>` to the roadmap branch, with a neutral price of 80%. The
item's page is `https://fulfilium.github.io/prodrome/#/todo/gh-<number>`.
When someone closes the issue, the workflow completes the item. When someone
reopens the issue, the workflow reopens the item.

A maintainer sets the price of an item with a comment on the issue. The
price uses the scale in the branch's
[`PRICING.md`](https://github.com/fulfilium/prodrome/blob/roadmap-data/PRICING.md):

```
/price 40
/price 30 --deadline 2026-10-15
/price ref gh-7
```

A pricing bot (Claude) suggests a price for every new issue. The workflow
stores the suggestion as a claim by the actor `pricing-bot`. A claim is a
suggestion that does not take effect. The viewer shows the suggestion next
to the item. The suggestion takes effect only when a maintainer replies
`/price accept`.

The workflows are `.github/workflows/mirror.yml` and `price.yml`. Both run
one binary, `prodrome-github`, which is deterministic and works offline. The
binary is in its own crate, `github/`. It is an optional part, like the
viewer: no code in `core/` or `cli/` knows about GitHub.
[`github-agent.md`](github-agent.md) explains the workflow and the limits
of trust in it.

## Add to the roadmap

To add or change roadmap items, open a pull request against the
`roadmap-data` branch, in the same way as for a change to the code. The
store has no policy, because the program that reads a store supplies the
policy (SPEC section 5), and a directory of objects is not a program.
Instead, the review process protects the roadmap:

- The name of every object is the hash of its bytes.
- No one changes or deletes an object.
- So the diff of a pull request contains exactly the objects that it adds.

Reviewers read these objects in the same way as code, and a maintainer
decides whether to merge them.

To see what one writer's events would change without applying them, pass
`--untrusted NAME`. The list then shows the confirmed state and the claimed
state side by side.
