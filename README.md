# Tokoro

[![verify](https://github.com/swissyai/tokoro/actions/workflows/ci.yml/badge.svg)](https://github.com/swissyai/tokoro/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-59d9e8)](LICENSE)

**A place for local models.**

> Tokoro is open-source alpha software. Commands, schemas, and runtime support may change before 1.0.

Tokoro helps you discover local models, choose one that fits, run it, connect local tools, and understand what is happening.

```text
Discover -> Choose -> Run -> Connect -> Understand
```

`OPEN-SOURCE ALPHA · NO USAGE TELEMETRY`

## Install

macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/swissyai/tokoro/releases/latest/download/tokoro-installer.sh | sh
```

Windows PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/swissyai/tokoro/releases/latest/download/tokoro-installer.ps1 | iex"
```

Then run:

```sh
tokoro
```

Release checksums are attached to each [GitHub release](https://github.com/swissyai/tokoro/releases).

## What Tokoro does

- Detects supported local runtimes, active endpoints, installed models, and available capacity.
- Helps choose and run a compatible model without calling an installed model active.
- Prepares connections for local tools and coding agents.
- Shows local inference, memory, runtime, and request evidence with its provenance.
- Runs local benchmark recipes and creates explicit, checked report files.
- Provides typed JSON commands for scripts and agents.

Tokoro currently recognizes MLX/MLX-DSpark on macOS, Ollama, LM Studio, llama.cpp, and OpenAI-compatible local endpoints. Runtime capabilities vary by platform and server.

## First run and controls

A fresh interactive install opens a short, skippable walkthrough. It does not run for JSON commands, redirected output, CI, or `TERM=dumb`.

```text
1-6    overview / measure / system / learn / setup / bloat
Tab    move focus
Enter  open or run the selected action
Esc    go back
/      search commands
m      choose a model
s      start or stop a supported server
c      connect a tool or agent
?      explain the current evidence
q      close, return, or quit
```

Run `tokoro commands --json` or `tokoro --help` for the current command surface.

## Agent interface

Machine-readable operations use the versioned `tokoro.agent.v1` envelope:

```sh
tokoro inspect --json
tokoro monitor --json
tokoro models --json
tokoro recommendations --json
tokoro agents --json
tokoro benchmark recipes --json
tokoro integrations --json
tokoro config show --json
```

Remote checks and downloads require explicit commands. Cleanup remains confirmation-gated and unavailable to agent scans.

## Palettes and visualization profiles

Palettes control color. Visualization profiles separately control layout, density, panel order, graph style, and bounded history.

Built-in profiles are `tokoro`, `operator`, `focus`, and `mono`:

```sh
tokoro visualization list --json
tokoro visualization preview focus
tokoro visualization apply operator
tokoro visualization schema --json
```

Custom profiles are local, data-only TOML. Validate and preview them before applying:

```sh
tokoro visualization validate ./my-view.toml --json
tokoro visualization preview ./my-view.toml
tokoro visualization apply ./my-view.toml --confirm --json
```

## Privacy

Tokoro has no account requirement and sends no usage telemetry. Runtime measurements stay on the machine unless you explicitly create and share an output.

Prompts and responses are not retained in session metrics or included in reports by default. Reports and handoffs exclude local paths, usernames, credentials, and unrelated process details. Nothing is uploaded automatically.

Network actions are explicit, including installer access, named model downloads, and optional public recommendation refreshes.

## Build from source

Tokoro requires the Rust toolchain pinned by this repository:

```sh
git clone https://github.com/swissyai/tokoro.git
cd tokoro
cargo run --release
```

Before submitting a change:

```sh
make verify
```

Please keep contributions focused, tested, respectful, and free of credentials or private machine data. Security issues should use [GitHub private vulnerability reporting](https://github.com/swissyai/tokoro/security/advisories/new), not a public issue.

## License and identity

Code is available under `MIT OR Apache-2.0`; the complete license texts are included in this repository. The Tokoro name, wordmark, Threshold mark, and official visual identity are reserved. Forks may use the code under its license but should not imply official endorsement.
