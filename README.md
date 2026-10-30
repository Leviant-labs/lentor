# Lentor

**By Leviant Labs. Less effort for you.**

Lentor is a local coding agent and an independent fork of
[OpenAI Codex](https://github.com/openai/codex). It is not affiliated with or
endorsed by OpenAI. The upstream [LICENSE](LICENSE) and [NOTICE](NOTICE) remain
intact; Lentor uses its own name and artwork.


## Prerelease workflow

- `main` holds the current Lentor prerelease checkpoint.
- `dev` is the working branch for CLI and backend development. Both branches
  start at the same checkpoint; promote reviewed changes from `dev` to `main`.
- In the independent Lentor checkout, `origin` is the Leviant Labs GitHub fork
  and `upstream` is OpenAI's repository. Upstream updates are integrated
  deliberately; another local checkout is not required.

This is not a stable release. Full namespace isolation, provider-setup failure
recovery, and the existing Bazel multiline-string build findings remain open.
The branding checkpoint passed 126 focused Rust checks and launcher checks;
broader Windows integration and locale-dependent checks are not all green.

## Run locally

This is a source checkout, not a published Lentor package. Build the existing
engine from `codex-rs`:

```sh
cargo build -p codex-cli --bin codex
```

Then run the launcher from the repository root:

```powershell
.\lentor.ps1
```

```sh
sh ./lentor
```

The launcher uses the already-built engine and does not rebuild on startup.
Set `LENTOR_BINARY` to an absolute engine path if using a different build output.
The internal executable and Rust crates retain their upstream names during this
first branding pass.

The launcher uses `~/.lentor` for its user profile, or the directory specified by
`LENTOR_HOME`. It overrides an inherited `CODEX_HOME` only for the engine process
and runs without the shared daemon. Your old user profile is not imported.

This is a partial namespace migration: the engine still uses its upstream
environment variables, project-local `.codex` configuration discovery, credential
service identifiers, and protocol names. Do not treat this pass as complete
isolation from project-level Codex configuration. Full path and environment
renaming needs a separate compatibility pass.

Configure an endpoint and API key in the first-run setup, or use `/provider`
inside a session. Use `/models` to choose an available model and advertised
reasoning effort. Keys entered in setup are stored in the OS credential store.

The proprietary upstream desktop frontend is not part of this repository.


## License

Lentor retains the upstream Apache-2.0 license and third-party attribution.
See [LICENSE](LICENSE) and [NOTICE](NOTICE).
