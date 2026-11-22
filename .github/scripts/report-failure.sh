#!/usr/bin/env bash
# Say on an issue or a pull request that a run of a roadmap workflow failed,
# so that a maintainer sees the failure there. Used by mirror.yml, price.yml
# and completes.yml, in a step that runs only when an earlier step failed.
#
#   report-failure.sh NUMBER VERIFY
#
# NUMBER is the issue or pull request that the run was about. VERIFY is the
# outcome of the job's step "Verify the store" (`success`, `failure` or
# `skipped`), so that the comment can say whether `prodrome verify` refused
# the store.
#
# The comment holds only text that this script writes: the workflow's name,
# the job's name and the link to the run. It holds no text of the issue or
# of the pull request, and it runs none. It needs the run's token in
# GH_TOKEN, with `issues: write` to comment on an issue, or
# `pull-requests: write` to comment on a pull request.
set -euo pipefail

number=$1 verify=$2
run="$GITHUB_SERVER_URL/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID"

if [ "$verify" = failure ]; then
  cause="\`prodrome verify\` found a fault in the store on \`roadmap-data\` at the step \"Verify the store\", so the run pushed nothing."
else
  cause="A step before the push failed, so the roadmap on \`roadmap-data\` may not record this yet."
fi

{
  echo "<!-- prodrome:failed-run -->"
  echo "**The \`$GITHUB_WORKFLOW\` run for this failed in the job \`$GITHUB_JOB\`.** $cause"
  echo
  echo "The log is at $run. When the cause is fixed, a maintainer can re-run the failed jobs."
} > failed-run.md
gh issue comment "$number" --repo "$GITHUB_REPOSITORY" --body-file failed-run.md
