## ADDED Requirements

### Requirement: Git-backed project state
All OpenSpec artifacts and implementation changes SHALL live in the `codex-upstream` Git worktree. A task MUST NOT be considered complete while its files exist only outside that repository or remain untracked without an explicit implementation commit.

#### Scenario: Repository identity
- **WHEN** project commands run
- **THEN** `git rev-parse --show-toplevel` resolves to the `codex-upstream` project root

#### Scenario: Reviewable state
- **WHEN** OpenSpec or Rust work is completed
- **THEN** the files are visible in Git status and are committed as reviewable repository history

### Requirement: Strict OpenSpec lifecycle
The project SHALL use the `spec-driven` OpenSpec schema. Proposal, specs, design, and tasks MUST be completed and validated before Rust implementation for this change begins.

#### Scenario: Pre-implementation validation
- **WHEN** implementation tasks begin
- **THEN** `openspec validate --all --strict` succeeds and all four change artifacts are complete

#### Scenario: Requirement traceability
- **WHEN** an implementation task is reviewed
- **THEN** it references one or more normative requirements or scenarios from this change

### Requirement: Rust-first implementation
Provider, authentication, transport, plugin-boundary, and app-server behavior introduced by this change SHALL be implemented in Rust. TypeScript may be used only as a compatibility reference or in an existing non-Rust surface and MUST NOT become the source of truth for the new runtime behavior.

#### Scenario: New runtime component
- **WHEN** a provider or authentication behavior from this change is implemented
- **THEN** its production logic and tests are Rust crates or modules in `codex-rs`

#### Scenario: Reference-only TypeScript
- **WHEN** the TypeScript CommandCode OAuth implementation is consulted
- **THEN** it informs test cases and behavior but no production dependency on that TypeScript file is introduced
