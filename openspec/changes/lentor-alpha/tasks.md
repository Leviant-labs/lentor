## Recovery fixes

- [x] Repair Bazel multiline strings and protect LF formatting on Windows.
  Verified formatter regressions and generated RBE BUILD query; pushed as
  `1f4331fc5` and `986043559` on the recovery branch.
- [x] Verify provider-switch recovery and four snapshots in an isolated checkout:
  four TUI and six backend cases passed, followed by scoped lint and formatting.
- [x] Commit and push the isolated recovery checkpoint as focused draft PRs:
  #2 (`2a256d404`), #3 (`fbcea68ee`), and #4 (`ecf748a63`). None is merged.
- [x] Verify the recovery fixes after integrating them with branding/protocol work:
  the final 2026-11-03 TUI run passes 24/24 selected cases, including single-flight,
  original-thread recovery, draft/FIFO preservation and manual-model snapshots.
  Evidence: `.worktrees/reviews/lentor-alpha-tui-goldens-final-2026-11-03.log`
  in the parent workspace. This is scoped integration, not full TUI coverage.

## Runtime and provider integration

- [x] Validate the pinned Respan codec compilation and request/stream contract on
  Windows: 44 scoped tests passed, zero skipped; independent codec review approved
  on 2026-10-31. This does not complete agent or live acceptance.
  On 2026-11-04, a combined run passed 48 codec fixtures and 16 install-context
  checks (64/64, zero skipped). Evidence in the parent workspace:
  `.worktrees/reviews/lentor-alpha-gateway-and-install-context-2026-11-04.log`.

- [ ] Partial: recover the shared runtime and rename the CLI while retaining
  required internal helper names; verify the integrated revision.
  The 2026-11-04 native Windows rebuild passed. Six core configuration-warning
  cases passed with reviewed snapshots. Of 172 selected CLI/login/doctor/lifecycle
  cases, 171 passed in the combined run; the remaining diagnostic case then
  passed after its project fixture and three command-remediation snapshots were
  updated to Lentor. This includes black-box discovery of only Lentor commands
  when both Lentor and Codex candidates exist on PATH. Full workspace coverage
  and the remaining identity inventory are still separate gates.
  Native managed-install metadata uses `LENTOR_MANAGED_BY_{NPM,BUN,PNPM,VITE_PLUS}`
  and `LENTOR_MANAGED_PACKAGE_ROOT`, isolated from legacy Codex variables without
  fallback. The focused CLI/Doctor run passed 146 selected tests (196 skipped):
  `.worktrees/reviews/lentor-alpha-managed-env-cli-2026-11-04.log`; the 16
  install-context checks are in the combined log cited above.
- [ ] Partial: finish and verify `LENTOR_HOME` isolation and setup across restart.
- [x] Review remaining optional streamlined OAuth page identity and its
  `codex://threads/new` desktop integration. Native CLI/TUI default to the legacy
  page (`codex_streamlined_login = false`), so do not change required external
  integrations or infer that ordinary Lentor login launches regular Codex.
  Both callback page snapshots were reviewed and the real local HTTP test passed
  on 2026-11-03. The optional page explicitly identifies upstream Codex desktop.
- [ ] Partial: integrate provider setup, model discovery and the Responses,
  Chat Completions and Anthropic Messages codecs.
  Chat/Messages dispatch is now connected locally after codec approval; the
  focused agent gate passes 14/14 cases, including approvals, MCP, cancellation
  and failure paths. Public app-server lifecycle now passes eight selected tests;
  integrated TUI now passes the 24-case focused run; live acceptance remains pending.
- [x] Let `/provider` activate an explicitly configured exact model when discovery
  fails or is empty. The TUI transition/snapshots and public lifecycle fixtures
  verify exact manual IDs without borrowing unrelated reasoning metadata;
  omitting a manual model leaves failed discovery blocked.
- [x] Extract bounded private session metadata as an inert history prerequisite:
  39/39 history tests passed, zero skipped, followed by scoped lint and formatting.
  Commit `ac596bb2f` is pushed in draft PR #5 on `alpha/05-native-history`.
  Clippy reports a `large_enum_variant` warning; no lint failure. Runtime activation
  is outside this checkpoint.
- [x] Extract the inert shared HTTP streaming transport and URL normalization:
  all 196 API/client library tests pass with zero skipped, followed by scoped
  lint and formatting. Commit `77e996b4e` is pushed in draft PR #6 on
  `alpha/06-translated-transport`. The restricted run's 15 socket-permission
  failures are superseded by the unchanged loopback-enabled passing run.
  This checkpoint does not enable selectable translated protocols.
- [ ] Record end-to-end Responses evidence: setup, discovery, inference, tool,
  continuation and failure behavior.
- [ ] Record equivalent end-to-end Chat Completions evidence.
- [ ] Record equivalent end-to-end Anthropic Messages evidence.

  Live package checkpoint, 2026-11-04: r3 Responses and Messages both passed
  discovery, exact-model streaming, actual plan-tool continuation, restart/resume,
  persisted tool outputs, credential/file audit, graceful shutdown, keyring
  deletion and temporary-directory cleanup. Exact model:
  `inclusionai/ling-3.1-flash-1019`; no fallback. Evidence in the parent workspace:
  `.worktrees/reviews/lentor-alpha-r3-concurrent-audit-live-responses-2026-11-04.json`
  and `lentor-alpha-r3-concurrent-audit-live-messages-2026-11-04.json`.
  These checks do not replace the separately required live failure-path evidence.
  The concurrent audit retains its 192 MiB, 20,000-file and 90-second limits;
  all 15 offline harness tests passed. Earlier serial audits timed out and remain
  failed attempts. Chat still fails streaming: bounded diagnostics identify
  additional generation/accounting fields that the strict adapter rejects.
  Its compatibility fix and fresh Chat acceptance remain pending.

## Focused agent boundary verification

- [x] Verify Chat/Messages rejected credentials and rate limits through real
  agent turns, including key redaction and no Responses fallback. Two test cases
  passed in combined run `6e1a624b-aa12-49ad-939a-5707b1c574bf`.
- [x] Verify cancellation before HTTP EOF withholds an already received patch
  call and leaves its target absent. One case covering both protocols passed.
- [x] Verify real approved and denied shell execution and matching continuation
  results for both translated protocols. One case covering four scenarios passed.
- [x] Verify real MCP execution and matching Chat/Messages continuation; the
  owned stdio-server helper was built and the combined fourteen-case run passed.
- [x] Verify all three protocols through app-server's public JSON-RPC lifecycle:
  eight selected tests passed on 2026-11-03, including six protocol/discovery
  scenarios with setup, exact model IDs, plan-tool continuation and restart/resume.
  This is mock-provider evidence, separate from live acceptance.

## Package and release

- [ ] Partial: verify hosted scoped CI, format, schemas and Bazel lock checks.
  Stable/experimental API exports, the config schema and generated Python SDK
  were refreshed on 2026-11-03. All 317 normal app-server-protocol library tests
  passed; the ignored schema-writer test was run explicitly during generation.
  SDK generation uses the bundled Python 3.12 because the pinned generator
  rejects Python 3.14. Hosted CI and final integrated lint/format remain pending.
  On 2026-11-04, 86 generated Python SDK contract/RPC/public-API cases passed and
  both alpha workflow files passed actionlint. Windows matrix tests now resolve
  the pinned V8 inputs, and CI explicitly selects Python 3.12; hosted execution
  of these local workflow changes remains unverified.
  The user authorized the complete default-feature Rust workspace suite on
  2026-11-04. Native integration helpers rebuilt successfully; `just test` is
  running and is still compiling dependencies. No full-suite pass is claimed.
  Parent-workspace logs: `lentor-alpha-full-suite-helpers-2026-11-04.log` and
  `lentor-alpha-full-workspace-2026-11-04.log` under `.worktrees/reviews`.
- [ ] Partial: build prebuilt-only Windows x64 archive and verify helper lookup,
  checksums, CLI help/version, app-server lifecycle, sandbox and home isolation
  with real integrated binaries.
  The local 2026-11-03 archive passed the real offline package smoke, including
  code-mode helper startup and unchanged fake Codex configuration/auth/session
  sentinels. Restricted-token execution read an isolated file and denied a write.
  It predates the final CLI wording and exec-server environment-name fixes;
  the replacement candidate must be rebuilt and verified separately.
  Replacement `lentor-windows-x64-alpha-local-20261104-r2.zip` now passes the real
  offline smoke and restricted-token read/write-denial probe, including temporary
  probe-directory removal. Archive and binary hashes are recorded in
  `.worktrees/reviews/lentor-alpha-r2-candidate-evidence-2026-11-04.json` in the
  parent workspace; an external archive checksum file is provided. The probe
  uses normal inherited Windows ACLs, matching the earlier PowerShell fixture;
  Python's private temporary-directory DACL prevented traversal in the initial
  probe attempt. These are local candidate results, not release approval.
  The r3 Windows candidate subsequently passed the local offline package smoke
  and restricted-token read/write-denial probe. Evidence in the parent workspace:
  `.worktrees/reviews/lentor-alpha-r3-candidate-evidence-2026-11-04.json`,
  `.worktrees/reviews/lentor-alpha-r3-offline-smoke-2026-11-04.log`, and
  `.worktrees/reviews/lentor-alpha-r3-sandbox-2026-11-04.log`. The candidate
  includes uncommitted source changes; it is not release approval.
- [x] Explicitly disable the upstream daemon-managed installer for this alpha.
  All 53 selected daemon cases pass, including production update/installer
  rejection before HTTP and retained local package preparation. Fake installer
  algorithms remain testable; a Lentor-managed updater is not implemented.
- [ ] Partial: validate the separate explicitly initiated trusted live-smoke
  workflow and bounded all-three-protocol harness against the integrated binary;
  no execution on untrusted pull requests and no credentials in logs.
  Live acceptance remains pending for the exact model
  `inclusionai/ling-3.1-flash-1019`, with no fallback. The first Responses attempt
  reached tool continuation and restart/resume but its final file audit timed out;
  it is not an acceptance pass. A diagnostic Chat attempt reported HTTP 429.
  Temporary keyring credentials were deleted and servers shut down. The revised
  harness preserves the original error, audits within explicit file/byte/time
  bounds, and reports temporary-directory cleanup; all 16 offline Python checks
  passed on 2026-11-03. Fresh live evidence is still required.
  The harness also strips inherited Codex access-token environment values;
  all nine offline harness cases passed after that addition on 2026-11-04.
- [ ] Refresh name availability and document release readiness; the 2026-10-29
  checks are observations, not reservation or trademark clearance.
  The 2026-11-04 exact `lentor` lookups returned 404 from crates.io and npm;
  fork-inclusive GitHub repository search found only `Leviant-labs/lentor` as
  an exact case-insensitive match. Evidence in the parent workspace:
  `.worktrees/reviews/lentor-name-check-2026-11-04.md`. These are point-in-time
  observations, subject to search indexing, not reservations or trademark clearance.
- [ ] Review all three protocol evidence records and distribution readiness
  before an explicit release decision. No automatic publication.
