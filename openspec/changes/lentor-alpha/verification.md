# Alpha verification checkpoint

This milestone is incomplete. Local implementation, automated fixtures and live
acceptance are separate evidence categories. Local candidate checks do not
establish release readiness or validate all uncommitted source changes.

## Latest 2026-11-04 checkpoint

The integrated source is being saved on `alpha/09-integrated-checkpoint`; the
earlier focused PR branches are preserved. This is a prerelease checkpoint.

- All 48 codec fixtures and 16 install-context cases passed before the final
  optional-accounting follow-up. The latest CLI/Doctor selection passed 146/146,
  including old-only, new-only and conflicting managed-install environments.
- The r3 Windows package passed offline lifecycle/helper/isolation checks and
  restricted-token read/write-denial checks. Responses and Messages then passed
  live discovery, streaming, actual plan execution and continuation, restart/resume,
  persisted tool outputs, exact model/provider/effort retention and cleanup.
  Both used `inclusionai/ling-3.1-flash-1019` through the Vercel gateway with no
  fallback. Compact evidence and package hashes are checked in at
  [evidence/2026-11-04-r3-live.json](evidence/2026-11-04-r3-live.json).
- The file audit now uses four workers with at most sixteen pending files.
  Its byte/file/time limits and credential matching are unchanged. All fifteen
  offline harness cases passed; successful Messages/Responses audits completed
  in 31.36/14.02 seconds. Earlier serial audit timeouts remain failed attempts.
- Chat diagnostics received HTTP 200 and identified optional root `generationId`
  and usage accounting extensions. The final small follow-up ignores those
  accounting extensions while retaining supported token counters and existing
  text/tool validation. The final scoped codec run passed 48/48 cases, zero
  skipped (nextest run `2c477b76-3414-4cc8-9445-310b34214a41`), including
  the gateway metadata fixture with preserved tool calls and token counts.
  A fresh native build and live Chat acceptance are still pending.
- The user authorized the complete default-feature workspace suite. Integration
  helpers rebuilt successfully. The `just test` attempt stopped before test
  execution in `glib-sys`: its GLib discovery command could not find `pkg-config`.
  The workspace includes the native GStreamer/GLib-dependent voice host. Native
  SDK setup and a new full-suite attempt remain open. Final scoped lint
  (`codex-provider-codecs`, `codex-cli`, `codex-install-context`) and `just fmt`
  completed successfully after the passing codec check. Tests were not rerun
  after lint/format. The source-checkpoint result is recorded at
  [evidence/2026-11-04-source-checkpoint.json](evidence/2026-11-04-source-checkpoint.json).
- The exact-name checks on crates.io and npm returned 404. GitHub search including
  forks found the existing `Leviant-labs/lentor` repository as its only exact match.
  These are point-in-time namespace observations, not reservations or trademark
  clearance. Hosted checks, dependency-advisory triage and release review remain
  open. This checkpoint includes no package release, merge or promotion.

## Earlier 2026-11-04 integrated checkpoint

- Stable/experimental API exports, config schema and generated Python SDK were
  regenerated. The 317 normal protocol library tests passed, and both ignored
  schema-generation invocations ran explicitly. The supported Python 3.12
  environment also passed 86 SDK contract/RPC/public-API tests.
- Six core configuration-warning cases passed with reviewed snapshots. The CLI
  run passed 171 of 172 selected cases; the final diagnostic case then passed
  after correcting its project directory to `.lentor` and reviewing all three
  command-remediation snapshots. The new command-discovery case proves Doctor
  reports Lentor candidates when both Lentor and Codex exist on PATH.
- The native Windows r2 rebuild, real offline package smoke and restricted-token
  read/write-denial probe passed. The archive remains a local candidate built
  from an uncommitted worktree. Its hashes and extraction path are recorded in
  `lentor-alpha-r2-candidate-evidence-2026-11-04.json` in the parent workspace's
  `.worktrees/reviews` directory. The sandbox probe uses ordinary inherited
  Windows ACLs; Python's private temporary-directory DACL blocked traversal in
  the initial fixture attempt. Only the disposable probe directories were removed.
- Live Responses acceptance passed against
  `https://ai-gateway.vercel.sh/v1`, exact model
  `inclusionai/ling-3.1-flash-1019`, with no fallback. It completed streaming,
  actual local plan execution and tool continuation, restart/resume, exact
  model/provider/effort retention, and another completed tool turn.
- Live Chat and Messages on that same r2 candidate failed in the first turn with
  a redacted disconnected-stream error. All three runs completed keyring cleanup,
  graceful server shutdown, temporary-directory removal and file/output scans
  with no credential found. These failures are not acceptance passes.
- Bounded direct wire diagnostics received HTTP 200 from both translated
  endpoints. They recorded only shapes/types/lengths/static classifications,
  not provider text or credentials. Observed gaps include standard Chat response
  metadata, gateway `provider_metadata`, and Messages thinking without signatures.
  A separate synthetic wire check confirmed unsigned thinking can be replayed
  to the same Messages endpoint. This is diagnostic evidence, not an agent pass.
- The compatibility patch passed all 47 codec fixtures after reproducing the
  new fixture failures on the previous decoder. It retains signed/unsigned
  thinking through bounded private state and exact native-block replay. Review
  then identified late Chat fingerprint/service-tier metadata as another edge
  case; that follow-up and its new fixture still require a fresh scoped result.
  The pinned Respan revision and canonical transcript representation are unchanged.
- Initial scoped CLI/core/protocol/codec lint and repository formatting completed.
  A remaining test `unwrap` warning was replaced with a descriptive `expect`.
  Final lint for the compatibility follow-up is pending. Both alpha workflows
  pass actionlint, while hosted CI remains unverified.

Live logs are `lentor-alpha-r2-live-{responses,chat,messages}-2026-11-04.json`
in that review directory. R2 does not contain the subsequent codec fixes. Rebuild
and reverify a separately named r3 candidate before extending live acceptance.
The user authorized the complete workspace suite without `--all-features`; it
will follow the current scoped fixes. No merge, promotion, publication or alpha-ready claim
has been made. CI advisories, account billing and the orphan MCP scenario remain
separate unresolved items.

## Verified implementation stages

### 2026-11-03 public app-server checkpoint

Run `4534a0f3-ee62-46a6-8e88-76b0a18021d2` completed **8/8 selected tests
passed**, 1,531 other tests excluded by the filter, with no nextest LEAK reports.
The process-isolated lifecycle fixture completed all six combinations of
Responses/Chat/Messages and available/unavailable model discovery. It exercises
public configure, model list, thread/turn start, actual local update_plan tool
continuation, shutdown/restart, thread resume and another turn. Captured POSTs
retain exact protocol endpoint, mixed-case namespaced manual model ID, original
tool identity/call ID and tool output before and after restart.

The failed predecessors exposed fixture setup errors: six obsolete credential
service expectations, an unnecessary experimental environments field, and a
missing SQLite state handle. The fixture now uses the real isolated SQLite
bootstrap already used by in-process server tests; NoopThreadConfigLoader still
serves its separate configuration role. No product resume bypass, skip guard or
protocol fallback was added. Log: `lentor-alpha-public-lifecycle-sqlite-2026-11-03.log`
in `.worktrees/reviews/`. The earlier six-of-eight and seven-of-eight runs are
superseded for this scope, and are not passing evidence.

The three offline package tests passed after the setup-guide update
(`lentor-alpha-package-tests-2026-11-02.log`), and the current workspace manifest
policy checker exited successfully (`lentor-alpha-manifest-policy-2026-11-02.log`).
These do not prove an actual Windows archive or hosted CI. Integrated TUI/manual
model snapshots, daemon policy tests, schemas, final lint/format, prebuilt package
and live acceptance remain separate. VercelAPI presence was checked without
revealing its value; no live call has run in this continuation.

The formatter repairs are committed and pushed as `1f4331fc5` and `986043559`.
Evidence includes seven formatter regressions, six targeted SDK formatter tests,
pinned buildifier checks and parsing the generated RBE BUILD target.

On 2026-10-31, `just test -p codex-provider-codecs` completed with **44 passed,
zero failed and zero skipped**, nextest run
`07df2ee6-db32-47fa-8b4d-6deaef2b072f`. The independent source review approved
the codec for integration, including the additional review for tool context
items that can exceed 1,000 tokens. The reviewed codec remains uncommitted.

The codec fixtures cover Chat reasoning and both function/freeform calls through
the next encoded request, exact call identity and raw inputs, Messages signed
and redacted thinking replay, provider-state scope, malformed/truncated streams,
tool withholding until clean EOF, freeform declaration validation, metadata
policy, and individual/aggregate wire-tool boundaries for both protocols.
The latest run also covers complete, ordered reasoning/text item lifecycles;
the earlier 40-test approval predates that fix. The runtime rereview approved
the mixed-item ordering fix, and the focused core run below verifies execution.
The 42nd case covers retained tool history with an empty active tool catalog,
including exact freeform input and rejection of newly generated undeclared calls.
The two subsequent cases cover signed thinking plus tool results as one
assistant turn, and deterministic projection of interleaved instruction updates
with exact signed/redacted block preservation and immutable canonical input.
They do not prove that the full agent or app-server preserves those contracts.

Local logs and independent review are retained under the sibling
`.worktrees/reviews/` directory as `lentor-alpha-codec-system-projection-fixed.log`,
`lentor-alpha-codec-gate-review.md`, and
`lentor-alpha-runtime-activation-review.md`. Earlier codec runs are superseded
by the 44-test projection run. The bounded historical-tool and request-control review
is recorded in `lentor-alpha-historical-tool-review.md`.

The focused transport/history/provider checks completed with **14 passed**, with
384 other cases excluded by the test-name filter, nextest run
`41a30c8c-a2cc-40e6-8906-2ecffdc5ec41`. This covers normalized endpoint paths,
Messages authentication/header precedence and discovery, transport failures,
and private native-state serialization/redaction. It does not exercise the
agent loop. Offline package/harness Python tests also passed: 15 tests.

The provider-codec scope completed `just fix -p codex-provider-codecs` without
warnings (`lentor-alpha-codec-clippy-final.log`). Cargo's fix lock server required
the same authorized Windows local-socket access. The final lint pass removes an
invariant-only `expect`, simplifies equivalent branches and method calls, and
documents expected fixture-helper panics. The required global `just fmt` also
completed on 2026-10-31 after its cache/download-access retry. The October checks
preceded lint/format, following repository test-order instructions. The new
November boundary fixtures form a subsequent implementation/verification stage;
milestone-wide lint, schema/lock and final-format checks remain separate work.

On 2026-11-01, combined focused core run
`6e1a624b-aa12-49ad-939a-5707b1c574bf` completed **14/14 passed**, with 2,101
other cases excluded and no LEAK reports. This includes the nine runtime cases
below plus Chat/Messages HTTP 401/429 failures with verified protocol-specific
authentication headers, synthetic-key redaction and no Responses fallback;
cancellation after receiving a complete tool call but before gated HTTP EOF
(no patch execution); actual approved/denied shell commands with matching
continuation results and a workspace execution marker; and real stdio MCP
execution preserving the original tool, call ID, and result in the next request.
The missing owned MCP helper exposed by the initial 4/5 run was built with
locked dependencies; the combined run supersedes that partial evidence.
Logs: `lentor-alpha-core-runtime-boundaries.log` and
`lentor-alpha-mcp-fixture-build.log` in `.worktrees/reviews/`.
The fork CI draft now builds explicit core-test helper binaries before running
core tests; that hosted workflow has not run at this revision.

On 2026-11-01, `just bazel-lock-update` and `just bazel-lock-check` both completed
successfully using repository-pinned Bazel 9.2.0. The lockfile is refreshed and
passes drift checking; this does not compile or test Rust through Bazel. The
resolver still warns that the root requests older `platforms` and `rules_cc`
versions than those selected transitively. No warning-suppression flags or
license-acceptance settings were added. Logs: `lentor-alpha-bazel-lock-update.log`
and `lentor-alpha-bazel-lock-check.log` in `.worktrees/reviews/`.

## Implemented, verification pending

- Typed provider-setup failure outcomes and TUI recovery; snapshot assertions
  and concurrent-attempt behavior pass four isolated cases. Cleanup run
  `03aef1b0-61c4-4382-bbef-2caef4eb42fc` reports **4/4 passed with no LEAKs**
  after fixture-only plugin-sync isolation. It includes a real saved-model
  change guard and preservation of the previous thread's draft/FIFO queue on
  successful handoff. Backend run `98ac3adb-5960-4e70-870a-d83073ef1031` passes
  all six selected setup/keyring cases. Scoped lint/format, checkpointing and
  integration of that recovery patch into this worktree remain. The new public
  all-three-protocol lifecycle fixture is authored but has not executed.
- Lentor command, TUI branding, profile/environment/credential isolation and
  helper/package lookup; final integrated snapshot and coexistence checks remain.
- Chat/Messages dispatch, configured HTTP transport, native-state persistence,
  local compaction and unsupported hosted-feature gates. The October 31
  focused core checkpoint, `c6a011e9-f3b4-42a0-9a78-3d4c7afe7b52`, passed **9/9** cases
  (2,101 other cases excluded). Passing execution covers text across all three
  protocols, translated text before HTTP EOF, malformed/truncated-stream failure
  without fallback, actual freeform patch execution, Chat and signed Messages
  reasoning with shell continuation and restart/resume, reasoning through resumed
  manual compaction, shell-history compaction, and explicit hosted-search rejection.
  This supersedes the earlier 8/9, 1/8 and 1/9 runs. Responses-only summary/context/
  replay/verbosity defaults are now restricted by protocol.
  The former Messages signed-thinking/tool/resume failure came from a late
  system/developer instruction after restart, confirmed by diagnostic run
  `f0c4ab04-90ce-4392-92fb-d3a64cb695b2`. The Messages-only instruction projection
  fix and its preservation fixture are implemented and pass the codec and agent
  checks. Log: `lentor-alpha-core-system-projection-loopback.log` in
  `.worktrees/reviews/`. The preceding sandboxed run selected seven provider cases
  and passed only the pre-inference rejection: all six HTTP cases timed out with
  no mock requests. The successful nine-case run used authorized Windows loopback
  access, without changing sandbox variables or adding skip guards. The November
  fourteen-case run above supersedes this nine-case checkpoint and adds approval,
  MCP, cancellation, credential rejection and rate-limit agent evidence. Public
  app-server and live acceptance remain separate; this is not alpha readiness.
- Fork CI, package assembler and trusted live harness. A real archive and package
  lifecycle smoke have not run. Remaining CI failures are separate from codec
  fixture results.

## Acceptance still required

Record actual agent/app-server behavior for Responses, Chat and Messages:
streaming, shell tools, approvals, freeform patch execution, MCP, cancellation,
restart/resume, compaction, credential failures, rate limits and redaction.
Complete schemas, scoped lint/final format and the explicitly authorized full
suite; Bazel lock refresh/drift checks pass separately above. Run the trusted Vercel harness against all three
protocols with exact model IDs and credential-cleanup evidence. Refresh name
conflicts before publication, build/smoke the Windows archive, and review the
whole milestone before alpha readiness or promotion to main.

Live acceptance uses the user's exact selected model
`inclusionai/ling-3.1-flash-1019` for all three Vercel protocol scenarios. The
harness and manual workflow are updated, and their eight offline harness tests
pass. No live call with this model is recorded yet; no model fallback is enabled.

The 2026-11-01 pre-publication name check found HTTP 404 at the exact public
package endpoints `https://crates.io/api/v1/crates/lentor` and
`https://registry.npmjs.org/lentor`. No published exact-name package was found;
this does not establish that either registry will permit registration. The
intended owner-scoped repository `https://github.com/Leviant-labs/lentor` exists.
The report is retained as `lentor-name-check-2026-11-01.md` in `.worktrees/reviews/`.


## 2026-11-02 recovery checkpoint

This update supersedes the pending statuses in the historical notes below.
The final isolated recovery source passed four selected TUI cases (run
`03aef1b0-61c4-4382-bbef-2caef4eb42fc`) and six selected app-server
setup/keyring cases (run `98ac3adb-5960-4e70-870a-d83073ef1031`), without
nextest LEAK reports. Scoped `just fix -p codex-tui -p codex-app-server`
finished successfully; required repository `just fmt` completed on November 2
with cache/download access, after a restricted-cache attempt failed.
Logs: `recovery-clippy-fix.log` and `recovery-format-elevated.log` in the
isolated checkout. Tests preceded final lint/format and were not rerun merely
for formatting.

The checkpoint is split, committed and pushed on the formatter baseline:

- `2a256d404`, backend outcomes, draft PR https://github.com/Leviant-labs/lentor/pull/2.
- `fbcea68ee`, session reconciliation/single flight, draft PR https://github.com/Leviant-labs/lentor/pull/3.
- `ecf748a63`, original-thread input preservation, draft PR https://github.com/Leviant-labs/lentor/pull/4.

The final stack matches the verified source. Intermediate commits were split
for review and were not independently rerun. Four snapshots are retained;
test homes, logs and unfinished protocol/branding work are excluded.
All PRs remain drafts; hosted CI and release readiness are not claimed.
The old `provider-recovery-pending.patch` is stale and must not be applied.

The TUI changes were integrated into the main runtime checkout without
reverting its additional wireApi and credential-reference handling. That
integrated source still needs its selected tests; standalone results do not
prove integrated behavior.


The initial hosted recovery CI snapshot is separately triaged in
`.worktrees/reviews/lentor-recovery-ci-2026-11-02.md`: missing `lentor-runners`
group, macOS billing/spending-limit startup failures, and Fence resolver setup
failure prevented several checks from running. Repository-wide manifest,
unlinked-file, spelling and dependency-advisory findings are outside the
backend PR diff, but have not all been independently reproduced on its base.
Linux jobs were still running; this is neither clean hosted CI nor proof of a
provider regression.

A small daemon alpha policy now rejects production updater entrypoints and
real installer HTTP transport while retaining fake internal algorithm fixtures
and explicit local-package preparation. Its focused behavioral tests and
existing daemon crate tests are in the coordinated integrated run; do not mark
this packaging gate passed before those results.

The first November 2 integrated build failed before test execution at
`codex-api/src/endpoint/translated.rs` because a reused `codex-client`
artifact lacked an export already present in main source. Dep-info for the
recovery-built variant omitted `translated_base_url.rs`, and the main source
was older than that artifact. The recovery and runtime checkouts share a
target directory, so timestamp-based freshness could not distinguish these
source revisions. No cache was deleted. Modified/new repository file timestamps
were refreshed to force a current-source rebuild; the refreshed compilation
passed the original missing-export point. Its test result is still pending.
Do not reuse a shared target across divergent worktrees without invalidating
source freshness, and do not count this failed compile as test evidence.
Logs: `lentor-alpha-integrated-lifecycle-2026-11-02.log` and
`lentor-alpha-integrated-lifecycle-refresh-2026-11-02.log` in `.worktrees/reviews/`.

The refreshed combined build subsequently stopped before test execution: recovery
integration had duplicated the transition module registration and placed its
first declaration before the snapshot macro. The early declaration was removed.
A concurrent TUI compile also reported `rustc-LLVM ERROR: out of memory`.
The next attempt is limited to app-server's integration target with one Cargo
build job. Changing the selected package set also changes dependency feature
unification, so that attempt rebuilds some dependencies; no tests are claimed
passed until its nextest execution summary is available.

The user explicitly approved deletion of the measured old debug symbols. Exactly
132 `.pdb` files dated before 2026-11-02 under this checkout's `target/debug`
and its `deps` directory were removed after path, extension, timestamp and size
checks, removing 3.2 GiB by summed file size. Available space rose from about
1.4 to 4.1 GiB during the active build. No executable, source, configuration or session was
removed. Candidate and result manifests are retained in `.worktrees/reviews/`
as `old-rust-symbols-cleanup-candidate.json` and
`old-rust-symbols-cleanup-result.json`. Incremental cache was measured empty.

Follow-up source inspection narrows the login-page concern: native ServerOptions
and TUI onboarding set `codex_streamlined_login = false` and select the legacy
local page. The automatic `codex://threads/new` launch is in the optional
streamlined page, enabled by app-client configuration. Ordinary native login
is not shown to launch regular Codex. Remaining page branding must be reviewed
without changing required external desktop/OAuth integrations. This is source
evidence, not an executed browser login acceptance result.

The single-job app-server attempt was interrupted after compiling its libraries
and test support, with Cargo exit code 1073807364 and no nextest execution
summary. After the user returned, no Rust processes remained. The same scoped
command was resumed without touching source freshness or deleting additional
caches; its new log is `lentor-alpha-public-lifecycle-resumed-2026-11-02.log`.
The preceding interrupted attempt is not test evidence. Available C: space was
about 9.3 GiB at resumption.
