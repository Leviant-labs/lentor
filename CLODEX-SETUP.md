# Clodex setup

From this checked-out repository, install the Rust toolchain and `just` following
the [source build instructions](docs/install.md). Python is also required by the
root justfile (`python` on Windows, `python3` elsewhere). Then run:

```sh
just codex
```

This runs Codex from the source checkout. No separate gateway or server process
needs to be installed. The custom provider uses the Responses API format; choose
an endpoint that supports it.

1. On first run, choose **Use a custom API endpoint**. In an existing session,
   enter `/provider` instead.
2. Enter the provider's API base URL and your API key in the setup form. Use Tab
   to switch fields and Enter to continue or save. The key is masked as you type.
3. On first run, choose the exact model from the discovered list. In a session,
   use `/models` (or `/model`) to select a model and its advertised reasoning
   level. If no reasoning levels are advertised, continue without one.

For example, a Vercel AI Gateway base URL is
`https://ai-gateway.vercel.sh/v1`; select `xiaomi/mimo-v2.6-flash` if it is
available to your account. Enter your own key in the form, not in chat.

The setup stores the key in the operating system's credential store, rather than
putting the raw key in ordinary configuration. If the OS keyring is unavailable,
setup returns an error. Existing provider configurations that use an environment
variable for their key remain supported.

App clients can use the app-server `modelProvider/configure` and `model/list`
methods for provider setup and model discovery. The closed desktop frontend is
not included in this repository.
