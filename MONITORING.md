# Monitoring posture

Tokoro monitors the local inference path from the host and runtime through the endpoint, client, and quality-review loop. It does not require Prometheus, Grafana, OpenTelemetry, or a collector.

The baseline is versioned as `local_inference_core.v1`. Inspect the current machine against it with:

```sh
tokoro monitor --json
```

The command reports capability coverage and current evidence separately. It does not calculate a universal performance score. Latency, throughput, memory, and queue budgets remain workload-specific and user-defined.

## Industry baseline

A useful local inference monitor needs nine layers:

1. **Lifecycle and availability**: readiness, loaded model and runtime identity, load failures, startup duration, restarts, and uptime.
2. **Request experience**: request rate and outcomes, TTFT, TPOT or inter-token latency, end-to-end latency, and queue/prefill/decode decomposition.
3. **Tokens and throughput**: input/output tokens, per-request decode rate, and aggregate system throughput under concurrency.
4. **Scheduler and cache**: running/waiting/swapped requests, queue duration, KV occupancy and eviction, prefix reuse, batching, preemption, and speculative acceptance.
5. **Host and accelerator**: host CPU/RAM/RSS/swap/storage plus device utilization, memory, temperature, power, throttling, and hardware errors.
6. **Quality regression**: versioned fixtures, human ground truth, repeatable execution, and deltas tied to model/runtime changes.
7. **Agent operations**: model and tool spans with duration, token, status, and error attributes; content capture remains explicit opt-in.
8. **Interoperability**: stable names, units, labels, provenance, aggregatable histograms, and vendor-neutral handoffs.
9. **Privacy and custody**: metrics without prompt content by default, bounded retention, and explicit export custody.

This baseline reflects the common surface across current vLLM, SGLang, TGI, OpenTelemetry, Prometheus, and NVIDIA telemetry documentation. It is a signal-coverage baseline, not a vendor threshold catalog.

## Where Tokoro sits

| Layer | Posture | Strong now | Important gap |
|---|---|---|---|
| Lifecycle | Partial | Live/loading/idle/error state, model/runtime/endpoint identity, runtime version | Time-to-ready history, restart/uptime counters, structured runtime errors |
| Request experience | Partial | Request stages, bounded metrics-only history, TTFT/TPOT/E2E benchmark percentiles | Passive request/error rates and queue/prefill/decode duration histograms |
| Tokens and throughput | Strong | Prefill/decode, per-request vs aggregate throughput, token provenance | Passive per-model traffic series outside measured workloads |
| Scheduler and cache | Conditional | Queue gauges, KV use/residency/evictions, prefix/batch/speculation when reported | Queue duration, preemption, and broad runtime parity |
| Host and accelerator | Partial | Host RAM, RSS, headroom, swap, CPU, storage, unified-memory semantics | GPU utilization, temperature, power, ECC/XID, portable per-device allocation |
| Quality regression | Partial | Private hashed fixtures and human pass/fail review | One-command fixture execution, deltas, and CI summaries |
| Agent operations | Partial | `tokoro.agent.v1`, local client setup, typed inspection | Metrics-only event stream, prepared mutations, model/tool span correlation |
| Interoperability | Intentional boundary | Checked Prometheus and OTLP JSON report handoffs | No live scrape endpoint, collector, logs, or distributed traces |
| Privacy and custody | Strong | Metrics-only session state, bounded history, checked explicit handoffs | Content-bearing tracing intentionally remains outside the default path |

`conditional` means Tokoro has a typed first-class field but the selected runtime may not report it. Missing evidence stays unavailable. It never becomes zero or a passing result.

## Current runtime differences

- **vLLM** exposes the broadest standard surface: request outcomes, TTFT and end-to-end histograms, queue time, running/waiting/swapped requests, KV usage, prefix queries/hits, and token counters.
- **SGLang** similarly exposes queue depth, cache hit rate, token usage, TTFT, TPOT, end-to-end latency, and scheduler state.
- **TGI** exposes queue size and duration, batch state, request duration, input/generated token distributions, and mean time per token.
- **llama.cpp** exposes a Prometheus endpoint and slot monitoring, but current public discussion still identifies a gap for portable per-device weights/context/compute accounting.
- **Ollama** exposes model inventory and active-process data, but a native passive inference `/metrics` endpoint remains a prominent open request. Tokoro must not fabricate scheduler or cache telemetry when Ollama omits it.
- **MLX and MLX-DSpark** can provide strong local rates, allocator, prefix, batch, and speculative evidence, but there is no cross-runtime standard equivalent to the mature vLLM/SGLang Prometheus surface.

## Operational cues

Tokoro converts evidence into deterministic language. Examples include:

- `No model is running - choose one to start.`
- `The runtime is responding, but the model is not ready yet.`
- `3 requests are waiting - first-token latency is exposed to queue pressure.`
- `KV cache is 92% occupied - new requests may trigger eviction, preemption, or queueing.`
- `Swap pressure is active - decode may stutter while pages compete with other processes.`
- `The model is ready, but no measured baseline exists.`

Every cue includes a code, severity, evidence source, interactive key, and agent-readable next action. Queue and cache cues appear only when the runtime reports those values.

## Next monitoring slices

The highest-value missing work is:

1. Record lifecycle transitions with startup duration and structured error reasons.
2. Add passive request outcome counters and standard latency histograms without retaining bodies.
3. Add Linux/Windows accelerator adapters, starting with NVIDIA utilization, memory, temperature, power, and error evidence.
4. Run private fixtures as a repeatable local regression suite and compare quality alongside performance.
5. Add a metrics-only JSONL event stream and preview/approval receipts for agent mutations.

A live Prometheus endpoint or OTLP exporter remains optional. Tokoro should first make these signals useful locally, then offer verified adapters without making an observability stack a prerequisite.

## Research basis

Checked with Firecrawl on 2026-08-22 using 15 targeted searches and 18 scraped primary or high-signal pages. A parallel recent-discussion pass returned 14 Reddit threads, 15 Hacker News stories, and live project records for vLLM, llama.cpp, and Ollama.

Primary references:

- [vLLM metrics](https://docs.vllm.ai/en/stable/design/metrics/)
- [SGLang production metrics](https://docs.sglang.ai/references/production_metrics.html)
- [Hugging Face TGI metrics](https://huggingface.co/docs/text-generation-inference/reference/metrics)
- [llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md)
- [llama.cpp per-device memory request](https://github.com/ggml-org/llama.cpp/issues/26129)
- [Ollama metrics request](https://github.com/ollama/ollama/issues/3144)
- [OpenTelemetry GenAI observability](https://opentelemetry.io/blog/2026/genai-observability/)
- [OpenTelemetry agent observability](https://opentelemetry.io/blog/2025/ai-agent-observability/)
- [NVIDIA DCGM exporter](https://docs.nvidia.com/datacenter/cloud-native/gpu-telemetry/latest/dcgm-exporter.html)
- [NVIDIA AIPerf server metrics](https://docs.nvidia.com/aiperf/server-metrics/ai-perf-server-metrics-reference)
- [Prometheus instrumentation practices](https://prometheus.io/docs/practices/the_zen/)
- [Prometheus histograms and summaries](https://prometheus.io/docs/practices/histograms/)
- [System-level inference benchmarking](https://arxiv.org/html/2508.10251v1)
- [Human review and golden datasets](https://www.braintrust.dev/blog/human-review-golden-datasets)
