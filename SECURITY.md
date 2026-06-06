# Security Policy

Cerul App is an alpha-stage local desktop application. Please do not report
security issues through public GitHub issues.

## Reporting

Email security reports to `security@cerul.ai` with:

- affected version or commit
- operating system and architecture
- reproduction steps
- impact assessment
- any logs or screenshots that do not expose private media or credentials

We will acknowledge reports as soon as practical and coordinate fixes before
public disclosure.

## Scope

In scope:

- desktop shell security
- local REST API exposure
- local data storage and credential handling
- packaged binary path handling
- update, signing, and installer behavior

Out of scope for this repository:

- Cerul Cloud server implementation
- billing, entitlement, and account-service internals
- third-party provider outages or provider-side vulnerabilities

## Credential Handling

Do not include API keys, Cerul Cloud tokens, private media, model cache contents,
or signing credentials in issue reports. Redact logs before sharing.

