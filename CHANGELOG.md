# Changelog

All notable changes to Tokoro are documented here.

## [0.8.0] - 2026-08-22

First public alpha.

### Local model operation

- Discover installed runtimes, responding endpoints, local targets, and checked Hugging Face artifacts.
- Distinguish installed, configured, loading, ready, idle, failed, and live states.
- Start compatible managed runtimes and configure detected local coding clients.
- Support macOS, Linux including Omarchy, and Windows-specific paths and capability semantics.

### Monitoring and measurement

- Add the versioned `local_inference_core.v1` monitoring posture through `tokoro monitor --json`.
- Add deterministic evidence-backed cues for model state, queue pressure, KV capacity, memory pressure, failed requests, and missing baselines.
- Track TTFT, TPOT, end-to-end latency, per-request throughput, aggregate throughput, queue/cache pressure, memory, and provenance.
- Add bounded 1/2/4/8 concurrency measurement, workload budgets, checked history, and comparable-run safeguards.

### Agents, quality, and custody

- Add the stable `tokoro.agent.v1` JSON interface and local client integration catalog.
- Add private content-hashed evaluation fixtures with explicit human review.
- Add deterministic `tokoro.report.v1` bundles and verified Markdown, JSON, CSV, Prometheus, and OTLP JSON renders.
- Add local `tokoro.handoff.v1` packs with dry runs, atomic writes, SHA-256 manifests, and no automatic uploads.

### Project

- Add cross-platform CI, reproducible release automation, dual MIT/Apache-2.0 licensing, contribution and security policies, and an agent guide.
