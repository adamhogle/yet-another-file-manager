# Branch Protection Checklist

Use this checklist when configuring branch protection for `main`.

## Required Rules

- [ ] Require pull request before merging.
- [ ] Require at least 1 approving review.
- [ ] Dismiss stale approvals when new commits are pushed.
- [ ] Require conversation resolution before merging.
- [ ] Require status checks to pass before merging.
- [ ] Require branches to be up to date before merging.
- [ ] Restrict who can push to matching branches.
- [ ] Do not allow force pushes.
- [ ] Do not allow deletions.

## Required Status Checks

Configure these required checks to match `.github/workflows/ci.yml`:

- [ ] `quality`
- [ ] `secret-scan`
- [ ] `docker-image`

## Optional Hardening

- [ ] Require signed commits.
- [ ] Enable merge queue.
- [ ] Restrict branch creation to admins/maintainers.
- [ ] Enable secret scanning and push protection.
- [ ] Enable Dependabot security updates.

## Operational Notes

- Keep `CODEOWNERS` up to date so reviews route automatically.
- If CI check names change, update branch protection immediately.
