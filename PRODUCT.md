# Tokoro product spine

**A place for local models.**

Tokoro makes the primary local path easy: find a model that fits, run it, connect a tool or agent, and understand what happens next. Monitoring, benchmarks, evaluation, and publishing support that path; they are not the headline or a prerequisite.

## The user jobs

### 1. I need a local model running

The default path is:

1. detect the machine, installed runtimes, local models, and available capacity
2. distinguish installed or configured models from models that are actually live
3. recommend compatible choices without claiming one universal best model
4. download a checked artifact or choose an existing target
5. start it where the runtime supports managed loading
6. confirm the endpoint is ready and expose one connection action

Every transition uses plain evidence-backed language: no model, downloading, starting, loading, ready, queued, pressured, failed, or stopped. A runtime state without evidence remains unknown.

### 2. I need an accurate reading

Tokoro must show measured values and their source:

- endpoint, loaded model, target path or model id, serving runtime, mode, drafter, quantisation, context limit, and sampling settings
- prompt tokens, cached tokens, prefill rate, TTFT, decode rate, inter-token timing, end-to-end time, output tokens, and errors
- speculative rounds, drafted, accepted, committed, cap, target verification work, and per-position acceptance
- process RSS, allocator active/peak/cache memory, KV cache, swap, CPU, thermal state, power mode, and headroom
- p50, p95, and p99 once enough requests exist
- whether a value is server-reported, log-derived, measured by Tokoro, or estimated

A number without provenance is not a benchmark result.

### 3. I need the best setup for my work

The flow is:

1. choose a workload: chat, code, agent, long context, batch, or custom
2. describe the constraint: fastest response, lowest memory, best quality, longest context, or lowest energy
3. inspect candidate model x quantisation x runtime x configuration combinations
4. run a short benchmark on the current machine
5. keep the winner and connect it to the user's coding tools

Tokoro works without a search account or API key. Its deterministic path checks public Hugging Face repositories, pins immutable commits, verifies safetensors hashes, and starts with small artifacts. Cached public local.ai recommendations are optional external evidence. Tokoro names the search adapter and never presents source metrics as local measurements.

### 4. I want to learn what is happening

Learning is a guided screen, not a glossary dump. `?` opens a selectable lesson. Each lesson has:

- one plain definition
- why it changes the user's experience
- the live reading to watch
- the current value or an honest unavailable state
- the next local action that teaches it

The first lessons cover prefill, TTFT, decode, KV cache, speculative verification, and bloat. `j/k` changes the lesson; the dashboard remains compact.

`tokoro monitor --json` exposes the same evidence as a versioned stack posture for agents. It separates industry-standard signal coverage from values available on the current runtime and emits deterministic cues. It assigns no universal performance score. See `MONITORING.md`.

## Benchmark recipes

Tokoro should provide simple local recipes before custom benchmarks:

- **Quick response**: short prompt, configured repeated runs, TTFT, TPOT, and decode
- **Coding turn**: realistic code prompt, 3 runs, end-to-end time
- **Long context**: 512, 2K, 8K, and 32K prompt sweep where the model allows it
- **Speculation check**: baseline versus speculative mode, acceptance and verified speed
- **Memory soak**: repeated requests over a fixed period, swap and allocator peak
- **Concurrency sweep**: simultaneous 1, 2, 4, and 8 request points with system throughput, per-request throughput, p95 latency, errors, queue pressure, KV pressure, RSS, swap, and headroom
- **Custom**: user prompt, output limit, temperature, runs, and concurrency

All recipes run locally by default. A public result bundle contains the workload shape, model id, hardware profile, context, timing data, and explicit measurement provenance. Prompt and response bodies remain excluded. A private fixture or hash can be attached separately when reproducibility requires content custody.

## Publishing targets

Tokoro exports a portable result bundle first. A saved report pack contains checked `tokoro.report.v1` JSON, an editable TOML recipe, deterministic Markdown, and CSV. The measured bundle is hashed independently from presentation, so changing the title, narrative, or visible sections cannot rewrite a measurement. Publishing is an explicit second step:

- local JSON, Markdown, CSV, or clipboard
- GitHub issue, gist, or repository report
- Hugging Face model-card evaluation results in `.eval_results/` or `model-index`
- local.ai or LocalScore handoff when an official submission path exists

The export must be useful even when no external account is connected. No result is uploaded silently.

`tokoro integrations` is the single typed catalog for local endpoint clients and report destinations. A prepared `tokoro.handoff.v1` directory contains target-shaped files, a relative-path manifest, content hashes, the checked source bundle, and a verification command. Preparation supports a side-effect-free dry run and is idempotent for the same target and bundle. Tokoro refuses to replace an unrelated directory.

Checked packs also form the durable run history. Tokoro indexes verified bundles without copying prompts into a second analytics store. Comparisons require matching workload and environment identities, including OS and Tokoro versions. They disclose configuration changes and refuse numeric deltas for unlike runs. A missing runtime version produces a warning rather than disappearing from provenance. A user may set workload-specific TTFT, time-per-output-token, end-to-end, throughput, memory, swap, or queue budgets. Tokoro does not ship a global vendor threshold.

Prometheus text and OTLP JSON are explicit checked handoffs for people who already operate collectors. Tokoro remains complete without either integration.

## Information architecture

`Discover -> Choose -> Serve -> Connect -> Measure -> Learn -> Publish`

The default path is short. Interactive startup may first show the 1.48-second Cursor Home + Threshold identity while local probes begin. Any key skips it. The identity never appears in typed CLI/JSON commands, redirected terminals, CI, or `TERM=dumb`; sound is opt-in.

1. **Overview** tells the user whether anything is running and gives one next action.
2. **Choose** (`m`) separates local targets, checked Hugging Face downloads, and sourced comparisons.
3. **Serve** (`s` or `Enter`) starts a compatible local target without freezing the UI.
4. **Measure** (`b`, `B`, or `r`) proves the setup on the current machine.
5. **Connect** (`c`) detects installed coding agents and copies the selected setup.

Screens: `1` overview, `2` measure, `3` system, `4` learn, `5` setup, `6` bloat. `/` or `Ctrl-K` opens a fuzzy command palette, so the key contract never consumes the bottom row of a small terminal. Normal 80-column terminals retain all four Overview, Measure, and System panels in stable asymmetric columns. Summary panels stop growing once their operational question is answered; inventory, workflow, request history, endpoint, process, and finding lists receive extra height. `Tab` moves a visible, numbered panel focus; `Enter` opens a panel-specific workspace with summary data, full evidence, provenance, and contextual actions; `Esc` returns to the unchanged grid. Expanded workspaces expose evidence as focus `1/2` and details/actions as `2/2`. Action rows are selectable with `j/k` and executable with `Enter`. `Tab` traverses both before opening the next panel in place. Wide terminals show both panes, while narrow terminals dedicate the full viewport to the active pane. Tokoro does not capture mouse events, preserving terminal-native selection without making any action mouse-dependent.

Runtime coverage:

- MLX/MLX-DSpark: managed local target loading and speculative telemetry
- Ollama: installed inventory, active model detection, memory/context metadata, and OpenAI-compatible connection
- LM Studio: responding endpoint discovery on its default local server port and connection snippets
- llama.cpp: OpenAI-compatible endpoint discovery plus `/health`, `/metrics`, and `/props` where exposed
- unknown local servers: endpoint discovery through configured ports and the generic OpenAI-compatible path
- LocalStudio / 0xSero: no stable public serving API is documented here; Tokoro supports it through the generic endpoint contract if it exposes `/v1/models`, rather than inventing a vendor integration

`h` checks the curated Hugging Face starter catalog. `l` shows cached public local.ai data and provenance inside Tokoro. `L` refreshes only when a supported public-web search adapter is detected. No local.ai account is part of the main path.

`P` opens panel setup. Panel visibility is persisted for detailed Measure/System views. Overview always keeps the device-capacity and runtime summary. Setup also selects a compact inference-signal focus (`balanced`, `latency`, `throughput`, `memory`, or `speculation`) while expanded evidence keeps every collected signal. History windows and request-ledger retention are bounded and configurable; both remain session-only and metrics-only. Durable tracking still requires an explicit checked report save.

Agents use the versioned `tokoro.agent.v1` interface: `commands --json`, `inspect --json`, `models --json`, `agents --json`, `recommendations --json`, `scan --json`, `benchmark`, `budget`, `report`, `eval`, `integrations`, `handoff`, and explicit `config` changes. Remote checks and downloads require explicit commands. Agent scans cannot remove files.

## Local quality loop

A person can turn a selected poor or failed request into a private `tokoro.eval.v1` fixture. The TUI records request metrics without content. The CLI can explicitly create a fixture from local prompt and expected-answer files; fixture JSON stores hashes rather than bodies. Human pass/fail review remains separate and editable. No automatic LLM judge becomes the default truth source.

## Bloat Check

Bloat Check borrows the useful part of the Department of AI Efficiency doctor: name the finding, show evidence, give a reversible next step, and never turn a guess into an automatic deletion.

Bloat starts with a bounded, local quick scan of the selected project and live runtime. It catches duplicate loaded endpoints, active swap, oversized context, poor prefix reuse, reconstructible build/cache artifacts, oversized instruction files, exact duplicate directives and skills, source concentration, transient work-state files, and literal agent launch scripts without a detected return gate.

Credential-shaped files are never opened. Agent traces remain outside the default quick-scan boundary. Explicit deep mode adds named harness config, global skill registry, and bounded content-free usage-shape checks over recent local sessions. It emits aggregate evidence only. `SAFE REMOVE` is reserved for deterministic reconstructible artifacts and requires a double-confirmed human action; context, source, skill, and workflow findings remain review-only.

## Platform contract

Tokoro runs as a native Rust TUI on macOS, Linux, and Windows. Omarchy follows the Linux path: XDG directories, Ghostty theme discovery, and Wayland data-control clipboard support. Windows uses profile and application-data directories plus `PATHEXT` command discovery. Crossterm provides raw input and alternate-screen behavior on Unix and Windows. System and process readings come from sysinfo on its supported Linux, macOS, and Windows targets.

Managed MLX serving remains macOS-specific. Linux and Windows prioritize observation of Ollama, llama.cpp, and OpenAI-compatible endpoints. A fresh install does not show a managed-start action unless a compatible server command is present. Linux and Windows model discovery includes GGUF files; direct Hugging Face checks use a portable safetensors starter rather than the MLX catalog. Host RAM and discrete device memory remain separate values.

## Agent workflow contract

Tokoro adopts the useful parts of current agent-plugin practice without becoming a plugin marketplace:

- one integration catalog instead of dozens of overlapping automatic triggers
- non-interactive JSON, dry runs, idempotent writes, and actionable errors
- relative manifest paths with traversal and symlink rejection
- whole sensitive fields excluded at capture rather than partially masked
- verification against real bytes and nested receipts instead of agent self-report
- pinned repo-root validation through `make verify`, `AGENTS.md`, and CI

Transcript memory, broad MCP credentials, cloud orchestration, and automatic uploads stay outside the workbench.

## Research basis

The layout follows Grafana's guidance that each dashboard answer a question, progress from general to specific, show problem records instead of every record, use explicit scales for unlike time series, and avoid misleading stacked graphs. Ratatui's `Length`, `Min`, and `Fill` constraints support bounded summaries plus panels that consume remaining space; fixed percentages are not the default.

Ollama's `/api/ps` contract makes currently loaded models, model memory, context, parameters, quantization, and expiry a distinct operational view. vLLM's metrics design separates server-level explanation from request-level outcomes and names queue time, prefill, TTFT, decode, inter-token latency, running/waiting/swapped requests, KV-cache use, prefix hits, and per-request speculative acceptance. K9s issue reports support full-height evidence views without copy-hostile borders. btop feedback supports one coherent memory composition instead of alternating, independently interpreted memory graphs.

Crossterm documents Unix and Windows terminal support. Microsoft documents the Windows alternate-screen buffer as exactly the window dimensions. Ollama documents native Linux and Windows runtimes. local.ai remains optional external evidence; LM Studio, llama.cpp, and Hugging Face retain their documented interfaces.

Primary references:

- [Grafana dashboard best practices](https://grafana.com/docs/grafana/latest/visualizations/dashboards/build-dashboards/best-practices/)
- [Ratatui layout constraints](https://ratatui.rs/concepts/layout/)
- [Ollama running-model API](https://docs.ollama.com/api/ps)
- [vLLM metrics design](https://github.com/vllm-project/vllm/blob/main/docs/design/metrics.md)
- [vLLM per-request metrics](https://github.com/vllm-project/vllm/blob/main/docs/features/per_request_metrics.md)
- [vLLM speculative acceptance metrics](https://github.com/vllm-project/vllm/blob/main/docs/features/speculative_decoding/acceptance_metrics.md)
- [Crossterm platform support](https://docs.rs/crossterm/latest/crossterm/)
- [Windows virtual terminal sequences](https://learn.microsoft.com/en-us/windows/console/console-virtual-terminal-sequences)
- [Ollama on Linux](https://docs.ollama.com/linux) and [Windows](https://docs.ollama.com/windows)
