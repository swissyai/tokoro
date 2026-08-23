# Tokoro publication boundary

Tokoro has two jobs that must stay separate:

1. help a person understand and improve local inference on their machine
2. help them publish a small, trustworthy result when they choose to

The dashboard is private by default. Nothing is uploaded automatically.

## Private by default

Never export these unless the user explicitly adds them:

- absolute paths, usernames, home directories, and project paths
- process names, PIDs, command lines, and unrelated applications
- server logs, raw prompts, responses, API keys, cookies, or local configuration
- full environment dumps, shell history, or private model directories
- personal referral or identity links unless the user selects that destination

## Safe benchmark result

A publishable result may include:

- operating system, hardware family, memory kind, and memory class, for example `Linux / x86_64 / 64 GiB system memory`
- operating system and runtime versions when relevant
- model id and revision, never a local path
- quantisation, context limit, serving mode, engine version, and explicit flags
- workload name, prompt token count, output limit, temperature, run count, and concurrency levels
- TTFT, prefill, per-request decode, system throughput, inter-token timing, end-to-end time, p50/p95/p99, errors, and token-count source
- speculative acceptance, draft cap, target verification work, KV cache, memory peak,
  swap status, and whether each value is measured, server-reported, or estimated
- benchmark date and a short reproducibility command with paths replaced by placeholders

Prompts and responses stay out of the result by default. A user may publish a redacted fixture
or a hash if they need reproducibility without disclosing content.

## Opinionated publication flow

`Measure -> Preview -> Save checked pack -> Edit recipe -> Render -> Dry-run handoff -> Prepare -> Verify -> Explicit publish`

A checked pack separates evidence from presentation:

- `bundle.json` contains `tokoro.report.v1` measured data and its SHA-256 receipt
- `report.toml` lets a person edit title, context, conclusion, and section visibility
- `report.md` and `runs.csv` are deterministic renders of that bundle and recipe
- `tokoro report verify` rejects a bundle whose measured data no longer matches its receipt

The recipe cannot override measurements. JSON remains the custody artifact; Markdown is the readable view; CSV is the portable table. Prometheus text and OTLP JSON are explicit checked handoffs. Rendering either format writes output or stdout only; Tokoro does not contact a collector.

Tokoro offers three clear outputs:

- **Private report**: checked local bundle, editable recipe, Markdown, and CSV saved only on this machine
- **Public result**: clean Markdown, JSON, or CSV with the safe fields above
- **Model evaluation**: Hugging Face `model-index` or `.eval_results/` when the result is a
  quality evaluation rather than a machine-speed report

Checked report history contains only packs with benchmark measurements. Deltas require matching workload and environment identities, including OS and Tokoro versions. Runtime version changes remain visible. If a runtime omits its version, Tokoro warns that an engine update may be part of the delta. Unlike runs receive a refusal instead of a misleading comparison.

Private eval fixtures use a separate `tokoro.eval.v1` boundary. Explicit prompt and expected-answer files remain in the private fixture directory. Fixture JSON contains hashes, metrics, and custody receipts, not content bodies. A human records pass or fail; Tokoro does not silently apply an LLM judge.

For a target-shaped export, `tokoro handoff prepare` creates a local `tokoro.handoff.v1` directory. GitHub receives a paste-ready issue or discussion body. Hugging Face receives a paste-ready model-card section. Prometheus and OTLP receive importable metrics files. Every target includes the checked source bundle and `HANDOFF.json`; no target performs an upload.

The handoff manifest contains only relative filenames, SHA-256 values, byte counts, media types, purposes, and the source bundle receipt. `tokoro handoff verify` checks every artifact and the nested report. Dry-run returns the exact file plan without writing. Repeated preparation of the same bundle and target is idempotent. Replacement is limited to an existing verified Tokoro handoff and requires `--replace`.

A publish destination is explicit. The preview must show exactly what will leave the machine. Editable narrative is visibly separate from measured fields. No silent telemetry, no automatic account linking, and no giant raw log attachments.

## Research contract

The report split follows four established patterns:

- [Model Cards for Model Reporting](https://arxiv.org/abs/1810.03993) pairs performance numbers with intended use, evaluation procedure, and context.
- [Interactive Model Cards](https://arxiv.org/abs/2205.02894) finds that language, visual cues, warnings, and guided interaction make technical documentation usable by non-specialists.
- [Publishing computational research](https://arxiv.org/abs/2001.00484) finds broad support for literate, inspectable reports whose analysis materials can be manipulated without losing the original publication.
- [Ten simple rules for non-visual, reproducible and accessible bioinformatics](https://arxiv.org/abs/2608.14400) argues that every visualization needs a structured decision record containing purpose, quantitative evidence, and uncertainty.

Tokoro therefore keeps measurements machine-readable, presentation editable, provenance visible, and every important chart recoverable as text.

## Destination policy

- local file and clipboard work offline
- GitHub preparation writes local issue/discussion Markdown and checked attachments; posting remains user-triggered
- Hugging Face preparation writes a local model-card section and checked attachments; Tokoro does not claim a registry submission
- public local.ai recommendations are read-only reference data until an official submission contract exists
- LocalScore is a browser or manual export until it publishes a supported API

## Writing style

Use short labels, strong defaults, and plain verbs:

- `Serve model`
- `Run benchmark`
- `Check model`
- `Configure agent`
- `Preview export`

Do not create a settings maze. Make the common path excellent and put uncommon controls behind
one deliberate detail view. The tone is confident and useful, not theatrical.
