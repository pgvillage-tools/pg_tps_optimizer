# Releasing

## Pull requests

Pull request titles must follow [Conventional Commits](https://www.conventionalcommits.org/), like
`fix: honour sslmode` or `feat(cli): add --foo`. The allowed types are `feat`, `perf`, `fix`,
`hotfix`, `bug`, `docs`, `test`, `revert`, `refactor`, `ci`, `build` and `chore`; a `!` marks a
breaking change (`feat!: ...`).

The type sets a label on the pull request, which decides where it appears in the release notes, and
which part of the version the next release raises:

| Title type | Label | Release notes | Version |
|---|---|---|---|
| `feat`, `perf` | `enhancement` | 🚀 Features | minor |
| `fix`, `hotfix`, `bug` | `bug` | 🐛 Bug Fixes | patch |
| `refactor` | `refactor` | 🧰 Maintenance | minor |
| `docs` | `documentation` | 🧰 Maintenance | patch |
| `ci`, `build` | `ci` | 🧰 Maintenance | patch |
| `chore`, `test` | `chore`, `test` | 🧰 Maintenance | patch |
| `revert` | `revert` | without a category | patch |
| `...!:`, or `BREAKING CHANGE` in the description | `breaking` | 💥 BREAKING CHANGES | major |

Dependabot pull requests (`chore(deps)`, `ci(deps)`, `build(deps)`, `docs(deps)`) are listed under
Maintenance as well.

## Making a release

[release-drafter](https://github.com/release-drafter/release-drafter) keeps a draft release up to
date with every pull request that is merged into `main`, with the next version and the release
notes.

To release, review the draft on the
[releases page](https://github.com/pgvillage-tools/pg_tps_optimizer/releases) and publish it. That
starts:

1. **release.yml**
    - builds the binaries for Linux (x86_64, ARM64) and macOS (Intel, Apple silicon), and attaches
      them to the release with their checksums
    - builds the container image for `linux/amd64` and `linux/arm64` on native runners, and pushes
      it to the GitHub Container Registry (`ghcr.io/pgvillage-tools/pg_tps_optimizer`) with the
      version tag and `latest`, using the `GITHUB_TOKEN` of the workflow (no secrets needed)
2. **build-and-publish-docs.yaml**: publishes the docs for version `X.Y.x` (patch releases update
   their minor version) and moves `latest` to it

## Documentation versions

The documentation site has a version per minor release (`0.3.x`, `0.4.x`, ...), selectable at the
top of the page; `latest` is the newest release.

To publish the documentation of a branch before a release, run **Build and publish documentation**
manually (Actions → Build and publish documentation → Run workflow). It publishes the selected
branch as version `dev`.
