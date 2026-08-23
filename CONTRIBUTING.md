# Contributing to Tokoro

Tokoro is a place for local models. Contributions should make the find, run, connect, or understand path simpler while preserving local custody and honest provenance.

## Before opening a change

1. Search existing issues.
2. Keep the change narrow and name the user-visible outcome.
3. For runtime work, link the public API or fixture that establishes the contract.
4. For telemetry work, state whether each value is measured, runtime-reported, log-derived, source-reported, or estimated.
5. Do not include prompts, responses, credentials, machine paths, personal data, or model files.

## Development

Use the pinned toolchain and run from the repository root:

```sh
make verify
```

For platform-sensitive work, also run the available native or cross-target checks:

```sh
make check-windows
```

Add a deterministic test for behavior changes. UI work should include relevant terminal sizes. Runtime parsers should use bounded fixtures and preserve unavailable values as unavailable.

## Pull requests

A pull request should include:

- the problem and intended user outcome
- the smallest relevant implementation summary
- tests and commands run
- privacy, platform, and compatibility effects
- screenshots only when visual behavior changed

Do not claim a platform or runtime was tested unless it was actually exercised. Compilation is not native runtime validation.

## Licensing

By submitting a contribution, you agree that it may be distributed under the repository's `MIT OR Apache-2.0` license. The Tokoro name and identity are covered separately by `TRADEMARKS.md`.
