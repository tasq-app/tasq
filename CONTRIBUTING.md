# Contributing to tasq

## Branches and pull requests

- `main` is always releasable. Nobody pushes to it directly: every change
  arrives through a pull request.
- Work happens on short-lived branches named after their purpose:
  `feat/spaces-tree`, `fix/notes-reload`, `docs/install`.
- Larger features are split into **stacked PRs**: each PR targets the
  branch of the one before it (`feat/a` → `main`, `feat/b` → `feat/a`, …),
  so each stays small enough to review. Merge them in order; after a PR
  merges, the next one is rebased onto `main` and retargeted to it.
- PRs are **squash-merged**. The PR title becomes the commit on `main`, so
  it must be a [conventional commit](https://www.conventionalcommits.org/)
  (a check enforces it):

  | Type | Use for | In the changelog / version |
  | --- | --- | --- |
  | `feat:` | something new for users | Features — bumps the version |
  | `fix:` | a bug fix | Bug fixes — bumps the version |
  | `perf:` | faster, same behaviour | Performance — bumps the version |
  | `docs:` `refactor:` `test:` `build:` `ci:` `chore:` | everything else | hidden, no release on its own |

  A breaking change adds `!` (`feat!: …`) or a `BREAKING CHANGE:` footer.

## Before opening a PR

```sh
cargo fmt
cargo clippy --all-targets
cargo test
```

UI changes update the snapshots: `INSTA_UPDATE=always cargo test --test
snapshots`, then review `git diff tests/snapshots/`.

## Releases

Versions follow [semver](https://semver.org/): `MAJOR.MINOR.PATCH`. While
tasq is young it stays on `0.x` — the leading zero says things may still
change — and the commits decide each bump:

| Since the last release | Next version |
| --- | --- |
| only `fix:` / `perf:` | patch: `0.2.0` → `0.2.1` |
| any `feat:` | minor: `0.2.1` → `0.3.0` |
| a breaking change (`feat!:` or a `BREAKING CHANGE:` footer) | minor too, until `1.0.0` |
| only `docs:`, `chore:`, `ci:`, `test:`, `refactor:` | no release |

The first versions were `0.1.0-alpha.1` … `0.1.0-alpha.9`, previews of
`0.1.0`. `1.0.0` is a deliberate step, taken with a `Release-As: 1.0.0`
footer once the data format and the keys are settled.

Releasing is automatic, driven by
[release-please](https://github.com/googleapis/release-please):

1. Each merge to `main` updates a **release PR** ("chore(main): release
   x.y.z") with the next version and its CHANGELOG entry, computed from the
   commits since the last release.
2. When you're happy with what's in it, **merge the release PR**. That tags
   the version, creates the GitHub release, builds the binaries for macOS,
   Linux and Windows, attaches them, and updates the Homebrew formula in
   [tasq-app/homebrew-tap](https://github.com/tasq-app/homebrew-tap).

release-please acts with the `RELEASE_PLEASE_TOKEN` organization secret (a
fine-grained token with *Contents* and *Pull requests* write
access to this repository); the Homebrew step with `HOMEBREW_TAP_TOKEN`
(*Contents* write on the tap).

Users then get it with `brew upgrade tasq` (or `brew install
tasq-app/tap/tasq`).

A specific version can also be forced from any commit merged to `main` with
a `Release-As: 0.2.0` footer.
