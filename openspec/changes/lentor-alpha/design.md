## Design

Use the existing Codex package layout with the Lentor entrypoint and retained
internal helper names. The Windows archive is assembled only from explicitly
prebuilt files; its per-file SHA-256 manifest allows repeatable inspection.
Offline CI verifies package mechanics with clean temporary profiles and no
provider keys. A separate trusted, explicitly initiated live workflow must
exercise the actual integrated client rather than an HTTP-only probe.

## Dependencies and gates

Runtime recovery and CLI rename precede the real package smoke. Home isolation,
provider setup, model discovery, protocol codecs and tools must be integrated
before the three protocol scenarios can count. Evidence must identify revision,
platform, protocol and observed setup, selection, inference, tool call, resumed
session and failure behavior. Do not record keys or request secrets in logs.

Refresh name-conflict checks, review legal and distribution readiness, and
review all three evidence records before deciding whether to publish. CI has no
automatic release, tag, publishing, or live-secret path.

## Translated freeform tools and context bounds

Chat and Messages transport freeform calls as JSON functions with one required
`input` string. The codec validates the envelope and restores the exact string;
the existing local tool parser, approval policy and executor validate and execute
it. Lark/regex definitions become explicit format guidance, not provider-enforced
constrained generation. This is the intended lowering contract, not a promise
to enforce an arbitrary grammar in the codec. Unknown or malformed format
declarations fail before the request. Native Responses keeps its own transport.

Each final translated wire tool, including namespace text, freeform guidance and
schema, is limited to 8,000 serialized UTF-8 bytes. The registry is limited to
256,000 bytes across serialized tools. Oversize declarations fail explicitly;
they are never truncated or omitted. These are conservative hard ceilings, not
a model-specific token estimate or a claim that every endpoint has that much
context. Existing trusted tool sources and local execution policies remain in
effect; the codec does not treat tool descriptions as permission grants. Items
over 1,000 tokens require the repository's additional context review.

Retained native reasoning is compatible only with its original provider scope.
An incompatible resumed transcript fails before provider inference and explains
how to select the original provider or start a new Lentor session. The transcript
is preserved; private reasoning/signatures are neither replayed to another scope
nor silently removed to make the request succeed.

## Gateway stream compatibility

Chat service metadata (`created`, `system_fingerprint`, `service_tier`) is
validated on every chunk, including late and usage-only chunks, then normalized
before the shared decoder. Creation times must be unsigned integers; fingerprint
and tier values must be null or strings no longer than 512 bytes. The gateway's
`provider_metadata` extension must be an object or null and fit 32 KiB per event.
It carries accounting/routing information and is not conversation content.
Other unknown fields keep their existing validation; tool arguments, reasoning
and signatures do not pass through this metadata-normalization path.

Messages thinking can begin without a signature. Preserve any later signature
fragments exactly; a clean unsigned block remains unsigned in bounded private
state and on same-provider replay. Do not fabricate a signature or discard the
thinking. An explicitly supplied empty or non-string signature remains invalid.
The pinned codec's generic Anthropic reasoning projection drops unsigned
thinking, so replay uses its public native-content API to retain the original
validated blocks. Existing endpoint/protocol identity checks and state bounds
remain in force. This does not add a named-effort/token-budget conversion.

## Messages instruction projection

Messages projects chronological system/developer updates into Anthropic's single
top-level system field, preserving instruction order and every nonprivileged
message's relative order. Signed/redacted reasoning and tool identities remain
unchanged; the canonical persisted transcript retains its original chronology.
A genuine instruction update changes the system prefix and can invalidate cached
conversation prefixes. This wire representation cannot retain the original
instruction position among user/assistant turns; local permissions and approvals
remain authoritative. Unsupported native instruction fields still fail encoding.
