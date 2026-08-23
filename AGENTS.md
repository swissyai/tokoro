# Tokoro agent guide

Tokoro is a place for local models. Preserve the simple find, run, connect, and understand path alongside local custody, typed agent interfaces, honest provenance, and cross-platform behavior.

## Working path

Run these commands from the repository root:

```sh
make check
make test
make verify
```

`make verify` is the completion gate. It runs format checks, Clippy with warnings denied, all Rust tests, and non-network CLI smoke checks.

## Architecture

- `src/runtime.rs` owns runtime adapters and coherent telemetry snapshots.
- `src/report.rs` owns immutable `tokoro.report.v1` measurements and deterministic renders.
- `src/handoff.rs` owns local `tokoro.handoff.v1` sharing packs and artifact verification.
- `src/monitoring.rs` owns `local_inference_core.v1`, stack posture, and deterministic operational cues.
- `src/cli.rs` adapts product operations to `tokoro.agent.v1` JSON.
- `src/ui.rs` renders state. Do not add filesystem or network work there.
- `src/input.rs` owns keyboard transitions and invokes App operations.
- `src/platform.rs` owns OS paths, executable discovery, and platform labels.

Keep module interfaces smaller than their implementations. Parse external data at module boundaries. Put long-running work on workers so the TUI remains responsive.

## Product contracts

- Installed is not active. Require runtime evidence before showing a model as live.
- Never blend host RAM, process RSS, device memory, model storage, or estimated context capacity.
- Keep measured, runtime-reported, log-derived, source-reported, and estimated values distinct.
- Prompts, responses, credentials, personal identity, absolute paths, PIDs, and command lines stay out of public reports.
- Monitoring posture separates capability coverage from evidence available now. It has no universal performance score.
- Queue and cache cues require runtime evidence; missing telemetry never becomes zero.
- Handoffs prepare local files only. They never collect credentials or upload.
- New destructive TUI actions require explicit confirmation. Agent commands remain read-only for cleanup.
- MLX managed serving stays macOS-only. Linux and Windows use portable observation paths.

## Change protocol

1. Name the data shape before adding branches.
2. Add or update a deterministic test.
3. Run the narrow test while iterating.
4. Run `make verify` before completion.
5. For cross-platform changes, also run `make check-windows` when the target is installed.

Do not add a new plugin system, broad MCP bundle, or automatic transcript memory to Tokoro. Prefer one typed command, one schema, and one verifiable artifact.
