# Release Process

This repository publishes the Rust crate `moenarch-geo-core` to crates.io from
the manual GitHub Actions workflow `.github/workflows/publish.yml`.

The workflow is intentionally limited to `moenarch-geo-core`. It does not
publish any other Rust crate, npm package, or JavaScript package, and it does
not require npm registry credentials.

## One-Time crates.io Setup

Before the workflow can publish, a crate owner must configure crates.io trusted
publishing for `moenarch-geo-core`.

Create a trusted publisher configuration on crates.io with these values:

| Field | Value |
| --- | --- |
| GitHub owner | `moritzbrantner` |
| GitHub repository | `geo-analysis` |
| Workflow file | `publish.yml` |
| Environment | `release` |

The GitHub repository must also keep a protected environment named `release`.
Maintainer approval for that environment is the release approval gate before the
workflow receives its crates.io publishing token.

crates.io trusted publishing can only be configured after the crate has an
initial owner record on crates.io. If crates.io requires an initial manual
publish before trusted publishing is available for this crate, perform that
bootstrap publish once, then add the trusted publisher configuration above.

## Publishing

1. Confirm the version and crate metadata for `moenarch-geo-core` are ready.
2. In GitHub Actions, run the `Publish Rust crate` workflow manually.
3. Approve the `release` environment deployment when prompted.
4. The workflow runs:
   - `cargo test -p moenarch-geo-core`
   - `cargo package -p moenarch-geo-core`
   - `cargo publish -p moenarch-geo-core`

The workflow uses GitHub OIDC and `rust-lang/crates-io-auth-action` to request a
temporary crates.io token. Do not add a long-lived crates.io API token or npm
registry token for this workflow.
