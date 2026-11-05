# Lentor setup

From this checked-out repository, install the Rust toolchain and `just` following
the [source build instructions](docs/install.md). Python is also required by the
root justfile (`python` on Windows, `python3` elsewhere). Build the existing engine:

```sh
cd codex-rs
cargo build -p codex-cli --bin lentor
cd ..
```

Run `.\lentor.ps1` on Windows or `sh ./lentor` on macOS/Linux. The launcher uses
`~/.lentor` (or `LENTOR_HOME`) for the user profile. See the [README](README.md)
for the remaining namespace migration limits.

This runs Lentor from the source checkout. No separate gateway service needs to
be installed. Choose the protocol supported by your endpoint: Responses,
Chat Completions, or Anthropic Messages. These are alpha capabilities; see the
verification tracker below for the remaining acceptance checks.

1. On first run, choose **Use a custom API endpoint**. In an existing session,
   enter `/provider` instead.
2. Enter the provider's API base URL and your API key in the setup form. Use Tab
   to switch fields and left/right on Protocol to choose the wire format.
   The optional Model field accepts an exact provider model ID. Press Enter to
   continue or save. The key is masked as you type.
3. If you left Model empty, choose an exact model from the discovered list.
   If discovery is unavailable, enter the exact Model ID in setup instead.
   In a session, use `/models` (or `/model`) to select a model and its advertised
   reasoning level. If no reasoning levels are advertised, continue without one.

For example, a Vercel AI Gateway base URL is
`https://ai-gateway.vercel.sh/v1`; the selected live-acceptance model is
`inclusionai/ling-3.1-flash-1019`, subject to provider availability. Live
acceptance with this model is still pending. Enter your own key in the form,
not in chat.

The setup stores the key in the operating system's credential store, rather than
putting the raw key in ordinary configuration. If the OS keyring is unavailable,
setup returns an error. Existing provider configurations that use an environment
variable for their key remain supported.

ChatGPT sign-in remains available through `lentor login` (browser flow) or
`lentor login --device-auth` (device-code flow). Credentials use Lentor's separate
profile. ChatGPT mode uses the built-in `openai` provider; if a custom provider
is selected, set `model_provider` back to `openai` and choose a supported model.

App clients can use the app-server `modelProvider/configure` and `model/list`
methods for provider setup and model discovery. The closed desktop frontend is
not included in this repository.

## Windows x64 alpha package

The archive is a candidate for testing, not a published release. Extract it to
its own directory, keep `bin`, `codex-resources`, and `codex-path` together, and
run `bin\lentor.exe --no-daemon`. This reuses the packaged binary without
compiling. Add that `bin` directory to PATH only if desired. Set
`LENTOR_HOME` to a separate directory if the default `~/.lentor` is unsuitable.
Verify the entries in `SHA256SUMS` before use; `manifest.json` records the
target, version and payload digests. Keep the included LICENSE and NOTICE.

To assemble from already-built x64 Windows inputs, put `lentor.exe`,
`codex-code-mode-host.exe`, `codex-command-runner.exe`,
`codex-windows-sandbox-setup.exe`, `codex-app-server.exe`, and `rg.exe` in one
directory, then run:

```powershell
python scripts/lentor_package.py --binaries C:\path\to\prebuilt --output C:\path\to\lentor-alpha.zip --version 0.1-alpha
python scripts/lentor_smoke.py C:\path\to\lentor-alpha.zip
```

Assembly neither builds nor downloads binaries and refuses to overwrite an
existing archive. The smoke starts the CLI and app-server with separate temporary
Lentor and Codex homes; it does not test a provider or sandbox execution. The
CI build obtains ripgrep using the checked-in upstream digest.
The upstream daemon-managed installer expects `bin/codex.exe`, while this package
provides `bin/lentor.exe`. Lentor alpha disables production daemon updates,
scheduled installer downloads, and legacy daemon migration. Local complete CLI
package preparation remains available, as do daemon stop and status commands.
Use the direct extracted CLI for the alpha candidate.

## Release readiness and name gate

On 2026-11-01, the exact public package endpoints
`crates.io/api/v1/crates/lentor` and `registry.npmjs.org/lentor` returned HTTP 404.
No published exact-name package was found at those endpoints; registration
availability is not established. The project's owner-scoped GitHub repository
`Leviant-labs/lentor` exists. These observations are neither a reservation nor a
trademark clearance. Refresh the name checks and record their results before
any release.

No alpha publication is approved by packaging or CI alone. Require reviewed
end-to-end evidence for all three selected protocols, including setup, model
discovery, inference, tools, continuation, and failure handling; require actual
Windows package and isolation smoke results as well. See
`openspec/changes/lentor-alpha/tasks.md`. Live provider checks must be explicitly
initiated in a separate trusted workflow, never by a pull request.

## Explicit live integration smoke

`Lentor trusted live smoke` is a manual `workflow_dispatch` workflow. Configure
the `lentor-live-smoke` GitHub environment with branch restrictions for `main`
and `dev`, required reviewers, and a `VERCEL_API_KEY` environment secret. Dispatch
from the exact revision of a successful `dev` push run of `Lentor CI`, supplying
its run ID. The workflow verifies the originating repository, event, branch,
workflow and revision before downloading the Windows candidate. It does not
build, publish or accept an artifact from a pull request.

All three matrix entries must pass using the exact
`inclusionai/ling-3.1-flash-1019` model. An absent model fails rather than
selecting another. Free availability is provider-controlled and may change.
The harness requires setup, discovery, streamed output, a persisted plan tool
result, restart/resume, and a second tool cycle with the original context.
Responses and Chat use `https://ai-gateway.vercel.sh/v1`; the Messages setup
input is `https://ai-gateway.vercel.sh/v1/messages`; the harness normalizes it to
the API base before configuration, matching the UI's normalization. The codec
routes requests to `/v1/messages`.

For the same explicitly authorized local test with an already-built CLI:

```powershell
python scripts/lentor_live_smoke.py --binary C:\path\to\bin\lentor.exe --protocol responses --model inclusionai/ling-3.1-flash-1019 --base https://ai-gateway.vercel.sh/v1
python scripts/lentor_live_smoke.py --binary C:\path\to\bin\lentor.exe --protocol chat --model inclusionai/ling-3.1-flash-1019 --base https://ai-gateway.vercel.sh/v1
python scripts/lentor_live_smoke.py --binary C:\path\to\bin\lentor.exe --protocol messages --model inclusionai/ling-3.1-flash-1019 --base https://ai-gateway.vercel.sh/v1/messages
```

The harness reads `VercelAPI` from its own environment, sends the key only to
setup over stdin, and removes it before launching app-server. It uses a
temporary `LENTOR_HOME`, deletes the created `lentor-api-providers` keyring
reference, scans bounded output and temporary files for key leakage, and emits
only a bounded result summary. The intended optional setup `wireApi` values
are `responses`, `chat`, and `messages`; persisted protocol mismatches fail.
Actual execution is pending the integrated binary and all three successful
summaries are required as evidence. These checks alone do not authorize release.
