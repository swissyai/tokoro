# tokoro

[![verify](https://github.com/swissyai/tokoro/actions/workflows/ci.yml/badge.svg)](https://github.com/swissyai/tokoro/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-59d9e8)](LICENSE)

**A place for local models.**

> Tokoro is open-source alpha software. Commands, schemas, and runtime support may change before 1.0.

Tokoro finds models that fit the current machine, runs them, connects local tools and agents, and explains what happens next. Monitoring, benchmarks, quality fixtures, and checked handoffs support that path without becoming prerequisites.

```text
Discover -> Choose -> Run -> Connect -> Understand
```

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

To build from source instead:

```sh
git clone https://github.com/swissyai/tokoro.git
cd tokoro
cargo run --release
```

For repository work, the pinned toolchain and root validation command remove guesswork:

```sh
make verify
```

The same command works in macOS, Linux terminals, and Windows Terminal or PowerShell. Tokoro starts in **Overview**. It shows the current chip, RAM use, RAM available for loading a model, free disk space in the configured models directory, runtime state, and one next action.

## First run

A fresh interactive install opens a three-step walkthrough after the launch identity. It explains the product path, asks what the user wants to do first, teaches only the shared navigation keys, and opens the matching starting view. `Esc` or `S` skips from every step. `q` quits. Nothing is uploaded, and completion is stored only in the local config file.

The walkthrough can be reopened from Setup or by searching `tour` in the command palette. An agent or dotfile can control the first-run state without parsing terminal cells:

```sh
tokoro config set onboarding.completed false
```

## Keyboard

```text
1-6  overview / measure / system / learn / setup / bloat
Tab  move to the next focus; expanded views traverse evidence, actions, then the next panel
Enter open the focused panel with full evidence and contextual actions
Esc  return to the stable panel grid
/    command palette (Ctrl-K also works)
s    start or stop the configured server
m    model hub: local targets, Hugging Face downloads, sourced comparisons
h    check small Hugging Face starters and pin their manifests
c    configure coding agents detected on this device
r    choose a workload recipe
b    run a quick local benchmark
B    run a prompt-length sweep
p    preview a clean local benchmark report
l    inspect cached public local.ai recommendations and their source
L    refresh local.ai data if a public-web search adapter is available
?    guided learning tied to live readings
j/k  select a lesson, process, model, or connection
P    customize detailed panels
g    refresh the bounded current-project Bloat scan
D    explicitly add local harness config and usage-shape checks
d    twice to remove a deterministic SAFE artifact
x    twice to confirm process termination
q    close detail, return to Overview, then quit
```

The selected panel shows its position, such as `1/4`, as well as an accent border. Expanded views have two explicit focus stops: `1/2` full evidence and `2/2` details/actions. The action rows are real controls: `j/k` selects one and `Enter` runs it. `Tab` then advances to the next visible panel; `Shift-Tab` reverses the order. Narrow terminals give each expanded stop the full viewport instead of hiding or squeezing the actions. Tokoro does not capture the mouse, so terminal-native text selection remains available; every Tokoro action remains keyboard-accessible.

## Agent interface

Tokoro exposes the same product vocabulary without requiring an agent to scrape terminal cells:

```sh
tokoro commands --json
tokoro inspect --json
tokoro monitor --json
tokoro recommendations --json
tokoro recommendations --refresh --json
tokoro models --refresh --json
tokoro models search "Qwen 7B" --json
tokoro models download mlx-community/SmolLM2-135M-Instruct-8bit --json
tokoro agents --json
tokoro agents setup pi
tokoro scan --json --project .
tokoro scan --deep --json --project .
tokoro config show --json
tokoro config set density compact
tokoro config set default-view bloat
tokoro config set onboarding.completed false
tokoro visualization list --json
tokoro visualization schema --json
tokoro visualization validate ./my-view.toml --json
tokoro visualization preview operator
tokoro visualization apply operator
tokoro visualization apply ./my-view.toml --confirm --json
tokoro config set observability.focus latency
tokoro config set observability.history-samples 160
tokoro config set observability.request-retention 64
tokoro config panel streams off
tokoro benchmark recipes --json
tokoro benchmark run "Concurrency sweep" --json --save
tokoro budget set "Quick response" ttft-p95-ms 800
tokoro budget set "Quick response" tpot-p95-ms 60
tokoro budget set "Concurrency sweep" system-tps 40
tokoro budget list --json
tokoro report init tokoro-report.toml
tokoro report history --json
tokoro report compare BASE_ID CANDIDATE_ID --json
tokoro report render bundle.json --recipe tokoro-report.toml --format markdown
tokoro report render bundle.json --format prometheus --output tokoro.prom
tokoro report render bundle.json --format otlp-json --output tokoro-otlp.json
tokoro report verify bundle.json
tokoro eval create regression-name --prompt-file prompt.txt --expected-file expected.txt
tokoro eval review FIXTURE_ID pass --note "Expected structure preserved"
tokoro eval list --json
tokoro integrations --json
tokoro handoff list --json
tokoro handoff prepare REPORT_ID github --output ./tokoro-github --dry-run --json
tokoro handoff prepare REPORT_ID github --output ./tokoro-github --json
tokoro handoff verify ./tokoro-github --json
```

The JSON contract is versioned as `tokoro.agent.v1`. `tokoro monitor --json` checks the current stack against the versioned `local_inference_core.v1` signal baseline, reports coverage separately from evidence available now, and emits deterministic operational cues. It produces no fake industry score, and workload thresholds remain user-defined. Inspection and scans are local and path-sanitized. `integrations` is the single catalog for local endpoint clients and checked report destinations, so agents do not need to infer capabilities from scattered help text. Remote operations are explicit: `models --refresh` contacts Hugging Face, `models download` downloads one named repository, and `recommendations --refresh` uses a detected public-web search adapter. Agents cannot delete Bloat findings.

## Environment variables

Tokoro has no required cloud environment variables. `.env.example` documents the optional values without containing credentials. These optional variables change local paths or explicit enrichment:

- `TOKORO_MODELS_DIR` sets the default model directory.
- `FIRECRAWL_API_KEY` enables an explicit public-web refresh for optional local.ai recommendations. Tokoro also reads it from the local Firecrawl config file when present.
- `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, and `XDG_STATE_HOME` select Linux and portable test locations.
- `APPDATA`, `LOCALAPPDATA`, `USERPROFILE`, and `PATHEXT` follow Windows conventions.
- `TERM=dumb` or `CI` disables the interactive launch identity.

Tokoro reads process environment variables. It does not automatically load `.env.example`.

No variable grants report-upload permission.

## Customization

Palette and visualization are separate. The classic terminal-native palette remains the portable default. First-party `tokoro`, `operator`, and `mono` palettes are also available, while discovered Ghostty themes remain optional.

```sh
tokoro config set theme classic
tokoro config set theme tokoro
tokoro config set theme operator
tokoro config set theme mono
```

The versioned `tokoro.visualization.v1` profile controls panel order, density defaults, dashboard layout, graph renderer, and bounded history window without containing colors. `tokoro` is the calm default, `operator` is dense, `focus` shows one evidence panel at a time, and `mono` uses ASCII graphs. Setup cycles the immutable built-ins without changing the palette.

```sh
tokoro visualization list --json
tokoro visualization preview focus
tokoro visualization apply operator
tokoro visualization schema --json
```

Custom profiles are strict, data-only local TOML rather than executable plugins. Validate and preview first; generated profile application requires an interactive confirmation or explicit `--confirm`. Invalid fields fail visibly, custom names cannot replace built-ins, and the active palette remains unchanged. See [VISUALIZATION.md](VISUALIZATION.md) for the complete schema and workflow.

Default-screen selection, panel visibility, compact inference-signal focus, history-window size, and metrics-only request retention persist in the platform config directory. Signal and request histories remain local to the current session; a durable measurement exists only after an explicit checked report save. The CLI exposes the same settings for agents and dotfile workflows.

### Launch identity

Interactive startup uses **Cursor Home + Threshold**. A block cursor claims the six letters in `TOKORO`; the vacated cells open a central threshold. The sequence lasts 1.48 seconds, runs while local probes begin, and skips on any key. Resize events recenter it. CLI/JSON commands, redirected terminals, CI, and `TERM=dumb` never show it.

```toml
[intro]
enabled = true
style = "cursor-threshold"
duration_ms = 1480
sound = "off"            # off | tokoro | PATH
motion = "full"          # full | reduced | none
slogan = ""
frames_path = ""
```

The built-in ident is opt-in. It uses six dry selections, one latch, and a neutral interval that opens from mono to stereo. It has no voice. Playback failure is silent. The Setup screen toggles launch, motion, and built-in sound. The CLI exposes the same controls:

```sh
tokoro config set intro.sound tokoro
tokoro config set intro.motion reduced
tokoro config set intro.enabled false
```

Custom visuals use `style = "custom"` and a local `frames_path`. Frames are printable ASCII blocks separated by a line containing `---`. Every frame must have the same width and height. Tokoro bounds custom files to 64 KiB, 32 frames, 160 columns, and 48 rows; invalid files fall back to Cursor Home without executing content.

## Model hub

`m` has three focused tabs:

1. **Local** shows the active endpoint and selectable local/runtime targets.
2. **Hugging Face** checks a small starter catalog, pins immutable commits, and downloads selected files.
3. **local.ai** shows cached public recommendations, the extraction method, source metrics, current RAM and disk capacity, and a copyable research note.

`h` opens Hugging Face directly. On macOS, Tokoro starts with sub-400 MiB MLX artifacts. Linux and Windows start with a portable 260 MiB safetensors artifact instead. This keeps the check and download path cheap without claiming that one model format serves on every runtime. On a sourced local.ai result, `f` searches Hugging Face for matching public safetensors artifacts and filters for MLX on macOS. Downloads use a staging directory. Tokoro requires public ungated repositories, safetensors weights, an immutable commit, reported file sizes, and LFS SHA-256 hashes. It verifies each weight file before installation. This checks the artifact, not the publisher's identity.

## Platform support

- **Omarchy and Linux:** XDG config/cache/state paths, Ghostty theme discovery, native Wayland clipboard data control, X11/XWayland fallback, Ollama, llama.cpp, and generic local APIs.
- **Windows:** `USERPROFILE`/`APPDATA`/`LOCALAPPDATA` paths, `PATHEXT` executable detection, Windows Terminal alternate-screen support, native clipboard, Ollama, llama.cpp, and generic local APIs.
- **macOS:** terminal defaults plus Ghostty themes, native clipboard, and optional managed MLX/MLX-DSpark serving.

Host RAM and discrete GPU memory are reported separately on Linux and Windows. Tokoro does not place device weights inside process RSS. Fresh Linux and Windows installs observe existing runtimes immediately; managed start is enabled only when a compatible command, such as `llama-server`, is found or configured.

Cross-target checks cover `x86_64-pc-windows-gnu`, `x86_64-unknown-linux-gnu`, and `aarch64-unknown-linux-gnu` in addition to the host build.

## Runtime coverage

Tokoro recognizes and observes:

- MLX and MLX-DSpark, including speculative decoding telemetry
- Ollama inventory and active models
- LM Studio local servers
- llama.cpp / `llama-server`
- other local servers exposing an OpenAI-compatible `/v1` API

External runtimes are reported honestly. Tokoro only manages loading where a compatible API exists.

## Agent setup

`c` detects supported commands and editor extensions, sorts installed agents first, and prepares them for the active endpoint. `Enter` copies the selected setup. Direct OpenAI-compatible agents are marked `D`; clients that need a protocol proxy are marked `P`. When no server is running, Tokoro uses the configured server port and labels the setup as prepared rather than live.

## Measure

Recipes run locally and keep their provenance visible:

- quick response
- coding turn
- long context
- memory soak
- prompt sweep
- bounded concurrency sweep at 1, 2, 4, and 8 requests

The concurrency recipe reports aggregate output rate, mean per-request decode rate, p95 end-to-end latency, errors, runtime-reported queue and KV peaks, server RSS, swap, and minimum headroom. Server usage tokens take priority. When a runtime omits them, Tokoro marks the system rate as a stream-frame estimate instead of treating it as exact.

Workload budgets are optional and user-defined. A budget can cap p95 TTFT, p95 time per output token, p95 end-to-end latency, server RSS, swap, or waiting requests. It can also require a minimum per-request decode rate or system throughput. A breach names the measured value and its evidence. Tokoro ships no vendor threshold as a universal default.

Tokoro distinguishes measured, server-reported, log-derived, and estimated values. Memory uses GiB/MiB. Speculative decoding is treated as lossless: the target verifies every committed token.

Measure now has four stable views:

- **Performance / Speculation**: live rates, scheduler state, runtime KV capacity, prefix reuse, batching, and verified draft acceptance
- **Inference Signals**: configurable compact focus over decode, prefill, TTFT, KV use, waiting requests, acceptance, and engine load; expanded evidence always shows all seven with independent scales
- **Inference Path**: one selected request from cached/fresh prompt tokens through first token, decode, scheduler pressure, verification, and finish; unavailable stages stay `?`
- **Request History**: an identity-stable, session-only metrics ledger with configurable bounded retention and no prompt or response bodies

The monitoring posture spans lifecycle, request experience, tokens/throughput, scheduler/cache, host/device, quality, agents, interoperability, and privacy. See [`MONITORING.md`](MONITORING.md) for the Firecrawl-checked industry baseline, current gap matrix, and primary references.

## Deterministic reports

`p` previews publication-safe output. `2` saves an editable report pack under local Tokoro state:

- `bundle.json`: immutable `tokoro.report.v1` measurements with a SHA-256 receipt
- `report.toml`: editable title, narrative, and section visibility
- `report.md`: deterministic Markdown rendered from the bundle and recipe
- `runs.csv`: portable measurement rows

The recipe can change presentation but cannot override measured fields. Re-render locally with `tokoro report render`; verify custody with `tokoro report verify`. JSON and CSV remain usable without a renderer, account, or network connection. Prompts, responses, absolute paths, usernames, contact details, process identifiers, and secrets are excluded.

`tokoro report history` indexes only checked packs that contain measurements. `tokoro report compare` produces deltas only when workload and environment identities match, including OS and Tokoro versions. It lists model, quantization, runtime version, mode, and context changes. When a runtime does not report its version, the comparison carries an explicit warning. Unlike runs are blocked rather than forced onto one leaderboard.

Prometheus text and OTLP JSON are explicit file or stdout handoffs from a verified bundle. Tokoro does not start an exporter, contact a collector, or require an observability stack.

## Integrations and checked handoffs

`tokoro integrations --json` lists two separate integration classes:

1. Local OpenAI-compatible clients such as Pi, OpenCode, Codex CLI, Claude Code, Cursor, and generic SDKs.
2. Report destinations prepared as local files for GitHub, Hugging Face model cards, Prometheus, OTLP, or generic Markdown/JSON/CSV use.

`tokoro handoff prepare` writes an atomic `tokoro.handoff.v1` directory. `HANDOFF.json` lists every relative artifact path, byte count, media type, purpose, and SHA-256. `tokoro handoff verify` checks the manifest, each file, and the nested `tokoro.report.v1` receipt. Repeating the same prepare command is idempotent. Replacing a directory is allowed only when the old directory is already a valid Tokoro handoff and `--replace` is explicit.

`--dry-run` returns the exact file plan without writing. Tokoro never gathers destination credentials and never uploads. The GitHub and Hugging Face targets produce paste-ready Markdown plus checked attachments; they do not claim an API submission happened.

## Local quality loop

A selected request in the expanded Inference Path or Request History can become a private eval fixture. The TUI saves metrics and provenance but not prompt or response bodies. `tokoro eval create` can explicitly copy a prompt and expected answer into the private fixture store; the checked JSON keeps only content hashes. `tokoro eval review` records a human pass or fail. Tokoro does not use an automatic LLM judge as the default truth source.

## Learn and diagnose

`?` opens short lessons for prefill, TTFT, decode, queueing, KV cache, prefix reuse, batching, speculative verification, and bloat. Each lesson explains why the metric matters, what to watch, and the current reading.

The **Bloat** screen starts with a bounded quick scan, then discloses selected evidence and actions. It checks duplicate loaded endpoints, active swap, oversized context, poor prefix reuse, reconstructible build/cache artifacts, oversized instruction context, duplicate directives and skills, source concentration, transient agent work files, and agent launch scripts without a detected return gate.

Only deterministic generated artifacts such as a Rust `target/`, dependency cache, framework build cache, `__pycache__`, or `.DS_Store` receive `SAFE REMOVE`. Review findings never auto-delete. Credential-shaped files are never opened, and removal requires `d` twice in the TUI.

`D` or `scan --deep` explicitly adds named Codex/Claude posture checks, installed-skill duplication and breadth, and bounded content-free usage shapes from recent local JSONL sessions. Deep results expose only aggregate counts and coded evidence, never prompts, responses, tool arguments, exact model IDs, or source paths.

## Public local.ai data

local.ai is an optional comparison source, not an account flow. `l` opens the cached reading inside Tokoro. The screen names the source as a public local.ai recommendation, identifies the search adapter, and separates source-reported intelligence, tasks per hour, and size from Tokoro's local measurements.

`L` refreshes only when Tokoro detects a supported public-web search adapter. Firecrawl is one adapter, not a requirement. Without one, local models, Hugging Face checks, downloads, benchmarks, and agent setup still work.

## Architecture

Tokoro is a library-backed binary with a thin `src/main.rs`. The crate is organized by product seams:

- `runtime`: Ollama and OpenAI-compatible adapters returning one coherent snapshot
- `device`: disk capacity for the configured models directory
- `huggingface`: manifest checks, pinned downloads, and SHA-256 verification
- `agents`: installed coding-agent detection
- `local_ai`: optional public-web lookup and local recommendation cache
- `monitoring`: versioned stack posture, current evidence, and deterministic operational cues
- `visualization`: strict `tokoro.visualization.v1` profiles, validation, and local installation
- `commands`: the stable command catalog shared by humans and agents
- `input`: keyboard state transitions behind one App interface
- `intro`: bounded launch frames, motion modes, and optional local sound playback
- `cli`: the `tokoro.agent.v1` adapter
- `ui`: rendering only; no filesystem or network work
- `bloat`: bounded discovery, evidence classification, and guarded removal
- `settings`: configuration persistence and terminal-theme adaptation
- `platform`: XDG/Windows paths, executable discovery, host labels, Wayland support, and managed-runtime defaults
- `report`: checked measurement bundles, comparable local history, budgets, and deterministic Markdown/JSON/CSV/Prometheus/OTLP rendering
- `handoff`: portable, atomic, SHA-256-checked sharing packs with no upload side effects
- `eval`: private content-hashed fixtures and explicit human reviews

## Privacy

The dashboard is private by default. Handoffs contain relative filenames only and reject traversal, symlinks, duplicate declarations, unlisted files, changed byte counts, changed hashes, and mismatched nested bundles. Exports exclude local paths, usernames, prompts, responses, PIDs, command lines, logs, and secrets unless explicitly added by the user. Nothing is uploaded silently.

See `DESIGN.md`, `PRODUCT.md`, `VISUALIZATION.md`, `MONITORING.md`, and `PUBLICATION.md` for the interface, product scope, profile contract, monitoring baseline, and publication boundary.

## Contributing and license

Tokoro is dual-licensed under `MIT OR Apache-2.0`. See `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, and `TRADEMARKS.md` before contributing or redistributing a branded build. The tracking-free landing page is maintained separately at [`swissyai/tokoro-site`](https://github.com/swissyai/tokoro-site).
