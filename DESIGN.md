# Tokoro interface direction

Tokoro is a keyboard-first local inference console. It should feel native to an Omarchy-style desktop: quiet, dense, deliberate, and useful without a mouse.

## Principles

- Use the terminal's active palette. Ghostty theme files are optional enrichment, not a runtime requirement.
- Use semantic colors: foreground, muted, accent, success, warning, error, memory, and weights.
- Use square borders, short labels, ASCII action text, and no decorative gradients.
- Keep Overview short. Show chip, RAM use, available RAM, model-disk capacity, runtime state, and the next action.
- Every action has a visible key, a clear verb, and a status result.
- Never block the event loop on network inference, model loading, or benchmarks.

## Launch identity

The launch sequence combines **Cursor Home** with **Threshold**. The character field is fixed rather than drifting. A block cursor claims `T O K O R O` from six exact cells; the empty cells then align into a doorway where the wordmark settles. The animation explains the name: Tokoro is the place made by the cursor.

The default is 1.48 seconds and contains no fake loading percentage. Local probes continue behind it. Any key skips it, resize events recenter the active frame, and the dashboard appears immediately at the duration cap. Full motion uses the six cell selections. Reduced motion uses only opening and resolved frames. No-motion mode shows the resolved mark. The intro is absent from typed CLI/JSON commands, redirected terminals, CI, and `TERM=dumb`.

Sound follows the same action. Six dry data selections lead to one low latch and a neutral perfect fifth widening from mono to stereo. The selected ident has no voice. Sound is off by default, playback failure is silent, and custom paths are passed to native players without a shell.

Custom visual files contain printable ASCII only. Equal-size frames are separated by `---`, bounded before loading, and rendered as data rather than executable code. Invalid custom frames fall back to the built-in sequence.

## Information structure

The home screen answers one question: **what should I do next, and is the current setup healthy?** It does not render every metric at once.

1. **Overview**: Model answers what is active; Capacity answers what fits; Inventory answers what is available; Next gives one state-dependent action.
2. **Measure**: Performance answers how the request is running and shows benchmark budgets or concurrency points; Inference Signals tracks decode, prefill, TTFT, KV use, queue depth, speculative acceptance, and engine load; Inference Path follows one request from cache reuse through first token and decode; Request History keeps identity-stable retained runs and can seed a private eval fixture.
3. **System**: Memory Stack accounts for host and device memory without double-counting; System Pressure shows material outside interference; Bloat Check shows bounded evidence; Endpoints / Provenance names what responded and how fresh each reading is.
4. **Learn**: a selectable lesson tied to the live value, not a static glossary modal.
5. **Setup**: theme, density, default screen, panel visibility, compact signal focus, history-window size, and metrics-only request retention.
6. **Bloat**: a quick-scan summary, selectable findings, evidence, and guarded cleanup for deterministic generated artifacts. `D` explicitly adds bounded local harness and usage-shape evidence.

Small terminals show one focused record or metric block. They never compress four panels into the same frame. At 76 columns, Overview, Measure, and System retain all four panels in stable asymmetric columns. Bounded summaries use a measured height; growing inventory, history, endpoint, process, and workflow lists receive the remaining rows. Measure places performance and the selected inference-signal focus together while the fixed request path sits above the growing request ledger. System places memory above endpoint provenance and pressure above Bloat findings. This avoids equal cards that turn extra height into empty interiors.

`Tab` and `Shift-Tab` move one explicit focus ring. The selected panel includes its position and `Enter open`, so selection is not communicated by color alone. `Enter` opens a richer workspace with summary data, full panel-specific evidence, provenance, and contextual actions. Expanded summaries use panel-specific row budgets rather than a shared percentage. Expanded views have two focus stops: evidence and details/actions. The action stop contains selectable rows; `j/k` selects and `Enter` executes the named action. `Tab` traverses both before advancing to the next visible panel; `Shift-Tab` reverses the traversal; `Esc` returns to the unchanged layout. Wide terminals show both stops side by side. Narrow terminals give the focused stop the full viewport. Model, command, and agent workspaces also use the full viewport. Breakpoints follow minimum readable pane widths rather than terminal-size labels. There is no permanent bottom key bar. Tokoro leaves mouse events with the terminal for native selection and remains fully keyboard-operable. `/` opens the command palette; `?` opens guided learning; `1` through `6` change screens.

## Model hub

`m` opens three tabs instead of one long scrolling document:

- **Local** shows the active endpoint and selectable local/runtime targets.
- **Hugging Face** checks small public MLX starters, pins an immutable commit, reports the download size, verifies safetensors LFS hashes, and installs through a staging directory.
- **local.ai** shows cached public recommendations with the extraction method, source metrics, current device capacity, and a copyable source note. It never presents external values as local measurements.

`h` opens the Hugging Face tab and starts the manifest check. `Enter` downloads or loads the selected artifact. Models larger than 1 GiB require `--allow-large` in the CLI. The starter catalog stays below 400 MiB so the download path can be tested cheaply.

## Human and agent adapters

The TUI and `tokoro.agent.v1` JSON commands share one product vocabulary. Humans use screens, keys, and the command palette. Agents use `commands`, `inspect`, `models`, `agents`, `recommendations`, `scan`, `integrations`, `handoff`, and `config`; they never scrape terminal cells. Handoff preparation has a dry run, deterministic file plan, idempotent same-bundle behavior, and a separate verification command. Remote checks and downloads are explicit. Agent scans remain read-only, while destructive TUI actions retain explicit confirmation.

`c` detects supported coding agents and editor extensions. Detected tools sort first. Direct OpenAI-compatible setups and proxy-required clients are labeled separately, and `Enter` copies the selected setup.

## Platform contract

- Linux, including Omarchy, uses XDG config, cache, and state directories. Ghostty themes under the user or system theme directories remain available.
- Linux clipboard support includes native Wayland data control and X11/XWayland fallback.
- Windows uses `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, and `PATHEXT`; executable detection includes `.exe`, `.cmd`, `.bat`, and `.com`.
- macOS keeps MLX/MLX-DSpark managed serving when installed. Linux and Windows observe Ollama and OpenAI-compatible runtimes without pretending MLX is available.
- Fresh Linux and Windows installs do not expose a false managed-start action. `llama-server` is selected when present; otherwise Tokoro remains an observer until a managed command is configured.
- Host RAM and discrete device memory are separate. Only Apple unified-memory systems nest reported model memory inside host memory accounting.
- Data bars and truncation marks use ASCII. Crossterm owns raw mode and alternate-screen behavior across Unix and Windows terminals.

## Live behaviour

- Server `/metrics` is cumulative telemetry. vLLM scheduler, KV-capacity, and prefix-cache metrics remain runtime-reported and are never blended with estimated memory ceilings.
- Server `/rounds` and the server log provide live request and speculative-round telemetry.
- Current request rates take priority over cumulative rates.
- Resize events are coalesced and the frame is redrawn after a short quiet period. Ratatui performs the actual terminal resize; Tokoro does not clear the screen for every resize event.
- Runtime discovery/telemetry is a single-flight adapter worker; missing or slow localhost ports never block input or drawing.
- Benchmark requests run on worker threads so polling, resize, and quit remain responsive. Concurrency is bounded to 8 requests.
- The runtime's completion-token usage drives system throughput when available. Stream-frame counts remain labeled estimates.
- Workload budgets come from local user configuration. An unavailable source cannot pass or breach a budget.
- Prefix hit rate, request cached/fresh tokens, runtime KV use, residency, and evictions remain separate from estimated context capacity.

## Publication boundary

The dashboard is private by default. The flow is `Measure -> Preview -> Save checked pack -> Edit recipe -> Render -> Explicit handoff`. A private save creates an immutable `tokoro.report.v1` JSON bundle, SHA-256 receipt, editable `report.toml`, deterministic Markdown, and CSV. Checked packs form local comparable history. Prometheus and OTLP JSON require an explicit render command and never contact a collector.

A shareable target uses a second boundary: `Checked report -> dry-run plan -> atomic tokoro.handoff.v1 directory -> verify bytes and nested bundle -> explicit user handoff`. Manifests accept only one-segment relative filenames. Verification rejects traversal, symlinks, duplicate declarations, unlisted or missing files, size drift, hash drift, and bundle-receipt mismatch. Tokoro does not hold destination credentials. The recipe controls title, narrative, and section visibility but cannot replace measured values. Public output removes paths, usernames, contact details, PIDs, command lines, prompts, responses, and secrets. See `PUBLICATION.md`.

## Repository verification

Repository changes end with `make verify`, which runs format, lint, test, and agent-facing CLI smoke checks through one root command. CI repeats the same gate and runs tests on Linux, macOS, and Windows.

## Key contract

`q` close detail, then Overview, then quit | `Esc` close detail or return to Overview | `Tab` / `Shift-Tab` panel focus | `Enter` open focused panel | `/` or `Ctrl-K` commands | `1-6` screens | `s` start/stop | `m` model hub | `h` Hugging Face | `c` configure agent | `r` workloads | `b` benchmark | `B` context sweep | `p` export preview | `l` sourced comparison | `L` refresh comparison when an adapter exists | `?` learn | `j/k` select | `x` twice to terminate selected process | `P` panel setup
