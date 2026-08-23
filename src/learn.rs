use super::App;

pub const TOPIC_COUNT: usize = 9;

pub struct Lesson {
    pub term: &'static str,
    pub definition: &'static str,
    pub why: String,
    pub watch: &'static str,
    pub current: String,
    pub next: &'static str,
}

pub fn lessons(app: &App) -> Vec<Lesson> {
    vec![
        Lesson {
            term: "prefill",
            definition: "Reading the prompt before writing.",
            why: "longer context increases the wait before the first token".into(),
            watch: "prefill tok/s and TTFT",
            current: format!("{} tok/s", app.real_pp.unwrap_or(0.0)),
            next: "Run a long-context recipe to see the tradeoff.",
        },
        Lesson {
            term: "TTFT",
            definition: "Time to first token.",
            why: "it is the latency people feel before a response starts".into(),
            watch: "the first-token reading on the active request",
            current: app
                .current
                .as_ref()
                .and_then(|request| request.ttft())
                .map(|duration| format!("{:.0} ms", duration.as_secs_f64() * 1000.0))
                .unwrap_or_else(|| "no active request".into()),
            next: "Use Quick response for a repeatable local reading.",
        },
        Lesson {
            term: "decode",
            definition: "Writing the response token by token.",
            why: "this is the sustained speed of chat and coding turns".into(),
            watch: "decode tok/s and inter-token time",
            current: format!("{:.1} tok/s", app.real_tg.unwrap_or(0.0)),
            next: "Compare the same workload across runtimes, not just model names.",
        },
        Lesson {
            term: "queue",
            definition: "Requests waiting for the scheduler to admit them.",
            why: "queue time can inflate TTFT even when device utilization looks healthy".into(),
            watch: "running, waiting, and swapped request counts",
            current: app
                .metrics
                .requests_waiting
                .map(|waiting| {
                    format!(
                        "{} running | {} waiting | {} swapped",
                        app.metrics.requests_running.unwrap_or(0),
                        waiting,
                        app.metrics.requests_swapped.unwrap_or(0)
                    )
                })
                .unwrap_or_else(|| "not reported".into()),
            next: "Use a concurrency-shaped workload before changing scheduler settings.",
        },
        Lesson {
            term: "KV cache",
            definition: "Stored attention state for the current context.",
            why: "it makes reuse possible but consumes memory as context grows".into(),
            watch: "runtime KV use, context tokens, memory ceiling, and swap",
            current: app
                .metrics
                .kv_cache_usage
                .map(|usage| format!("{:.1}% used", usage * 100.0))
                .unwrap_or_else(|| format!("{}k tokens; capacity not reported", app.ceiling.current_tokens / 1000)),
            next: "Treat the lower of model window and memory ceiling as the real limit.",
        },
        Lesson {
            term: "prefix reuse",
            definition: "Reusing cached prompt blocks instead of prefilling them again.",
            why: "stable shared prefixes can reduce first-token work without changing output".into(),
            watch: "prefix queries, hits, partial hits, and reused tokens",
            current: match (app.metrics.prefix_hits, app.metrics.prefix_queries) {
                (Some(hits), Some(queries)) if queries > 0 => {
                    format!("{:.1}% hit rate", hits as f64 / queries as f64 * 100.0)
                }
                _ => "not reported".into(),
            },
            next: "Repeat the same long system prefix and compare TTFT with provenance.",
        },
        Lesson {
            term: "batching",
            definition: "Serving multiple requests together in one model step.",
            why: "it can raise total throughput while reducing each user's token rate".into(),
            watch: "batch size, waiting requests, aggregate throughput, and tail latency",
            current: app
                .metrics
                .batch_max
                .map(|maximum| {
                    format!(
                        "max {} | {} requests across {} batches",
                        maximum,
                        app.metrics.batch_requests.unwrap_or(0),
                        app.metrics.batch_batches.unwrap_or(0)
                    )
                })
                .unwrap_or_else(|| "not reported".into()),
            next: "Compare concurrency 1 with a bounded multi-request workload.",
        },
        Lesson {
            term: "speculation",
            definition: "A drafter proposes; the target verifies.",
            why: "accepted proposals can improve speed without changing the target's answer".into(),
            watch: "acceptance, cap, committed tokens, and verified rounds",
            current: app
                .metrics
                .draft_acceptance
                .map(|value| format!("{:.1}% accepted", value * 100.0))
                .unwrap_or_else(|| "not reported".into()),
            next: "A high acceptance rate is useful only if verified throughput improves.",
        },
        Lesson {
            term: "bloat",
            definition: "Work or state that adds cost without improving the accepted result.",
            why: "large contexts, duplicate servers, and repeated loops can hide inside a fast model"
                .into(),
            watch: "Bloat Check findings and their evidence",
            current: if app.bloat.findings().is_empty() {
                "clear".into()
            } else {
                "review".into()
            },
            next: "Fix one finding, rerun the same benchmark, and keep the change only if the result holds.",
        },
    ]
}
