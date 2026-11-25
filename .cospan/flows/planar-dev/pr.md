Finalize and open the Pull Request for the verified working tree:

1. Confirm you are on a dedicated feature branch (never `master`), stage all intended changes, and commit with Planar's Conventional Commit prefix (`CONTRIBUTING.md`).
2. Run `npm run pre-pr` on the final commit so all gates pass and the `HEAD`-bound pre-PR marker is written.
3. Push the branch and open a concise PR via `gh pr create` following `.github/PULL_REQUEST_TEMPLATE.md` (include `Closes #<n>` if an issue number was given, plus Review Tier, verification proof, and how to revert).
4. Return the PR URL and a brief summary.
