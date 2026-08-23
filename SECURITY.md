# Security policy

## Supported versions

Security fixes are applied to the latest tagged alpha release and the default branch. Tokoro is pre-1.0; older alpha releases may not receive backports.

## Reporting a vulnerability

Please use GitHub's private vulnerability-reporting flow for the repository rather than a public issue. Include:

- affected version and platform
- the smallest reproducible case
- expected and observed behavior
- likely impact
- whether credentials, local files, prompts, responses, or network actions are involved

Do not include real credentials, private prompts, or private model content. Use synthetic fixtures.

## Security boundaries

Tokoro:

- probes configured localhost endpoints
- reads bounded local runtime and system evidence
- can explicitly download a named public Hugging Face repository
- can explicitly use Firecrawl for optional public recommendation refreshes
- writes local configuration, private eval fixtures, reports, and checked handoffs
- can start or stop a configured local server when supported

Tokoro does not silently upload reports, collect publishing credentials, retain prompt or response bodies in session telemetry, or grant cleanup authority to agents.

Prepared Prometheus and OTLP files are offline handoffs. Tokoro does not start a collector or exporter.

## Disclosure

Please allow a reasonable remediation window before public disclosure. Confirmed reports will receive a private acknowledgement and a public credit when desired and appropriate.
