# sim-cli

sim-cli is the command-line bootloader repository for SIM.

## Crates

- `sim-cli` provides the `sim` binary.
- `sim-cli-core` provides the command entry API used by the binary.
- `sim-lib-repl` provides the loadable `cli/main/repl` entry point and the
  read-eval-print core used by that entry point.
- `sim-view-tty` is a loadable terminal (CLI/TUI) view/edit surface: it projects
  a `Scene` to stable ASCII and reduces terminal key input to validated `Intent`
  values, so a terminal is a runtime-loaded surface rather than a baked
  subcommand. Both directions are pure and testable without a tty.

## Command Surface

The checked binary surface is:

```bash
sim --help
sim --version
sim --codec lisp --load symbol:demo run --payload-for-loaded-libs
sim --codec lisp --load symbol:demo -- run --payload-for-loaded-libs
```

The parser accepts `--codec`, repeated `--load`, `--list`, `--inspect`,
`--eval`, `--script`, `--stdin`, and `--` as an explicit payload boundary.
The first non-flag token also starts the payload for the selected loaded
library. Library sources use `symbol:`, `path:`, `url:`, `bytes:`, `host:`, and
`crates.io:PACKAGE@REQ` prefixes. Domain behavior is not built into the binary.

At boot, `sim` loads the selected codec as the first library. The default codec
name is `lisp`, which maps to the library symbol `codec/lisp`; `--codec json`
maps to `codec/json`. The codec resolves through an explicit matching `--load`,
then catalog/cache sources, then `crates.io:sim-codec-NAME@^0.1`, then an
available host codec. Load receipts mark that first library as the boot codec.

After loading, `sim` hands the envelope to a loaded library with a resolved
function export under `cli/main`, such as `cli/main/demo`. Entry points from
explicit `--load` libraries take precedence over the boot codec entry point. The
returned value maps to the process exit code by truthiness: truthy is `0`, false
or nil is `1`.

`crates.io:` resolution belongs to `sim-cli-core`, not the kernel. The resolver
checks a CLI-owned cache directory first: `SIM_CLI_CACHE_DIR` when set, then
`$XDG_CACHE_HOME/sim/libs`, then `$HOME/.cache/sim/libs`. Cached package
artifacts resolve to kernel `path:` sources. A local registry fixture can seed
the cache for offline use. When built with `registry`, the binary can fetch a
missing package artifact from the explicit `SIM_GIT_REGISTRY_ENDPOINT`
git registry artifact endpoint and then store it in the same cache. When no explicit
catalog source handles a symbol, `codec/lisp` maps to `sim-codec-lisp@^0.1`,
and an unqualified symbol such as `demo` maps to `sim-lib-demo@^0.1`.

The `dynamic-native` and `wasm` features compose additional loader mechanisms
into the thin binary. `dynamic-native` loads platform dynamic libraries from
local paths; `wasm` loads `.wasm` modules through the wasm ABI runtime. Both
loader paths accept `site` exports: a guest binds an opaque placement target
under a symbol, and the agent model catalog gives that value `EvalSite`
semantics outside the kernel.

## Local Package Checks

The package manifests use version requirements for constellation crates and keep
source overrides outside git. Local development and package listing use the
generated meta-workspace from the control checkout:

```bash
sh bin/simctl meta-build
cargo package --manifest-path .meta-workspace/Cargo.toml -p sim-cli-core --allow-dirty --list
cargo package --manifest-path .meta-workspace/Cargo.toml -p sim-cli --allow-dirty --list
```

From this checkout, point cross-repo recipes at that generated manifest:

```bash
SIM_META_WORKSPACE_MANIFEST="$CONTROL_ROOT/.meta-workspace/Cargo.toml" sh recipes/publish-readiness/package-list/setup.sh
```

Temporary `.cargo/config.toml` files may carry local `[patch.crates-io]`
entries for unpublished sibling crates. Keep those files local; release
packaging uses crates.io version requirements.

## Validation

These commands run in the constellation workspace; only `sim-kernel` builds from a lone clone today (see `DEVELOPING.md` in `sim-sdk`). A single-repo build lands with the first crates.io publish.

```bash
cargo fmt --check && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo doc --workspace --no-deps
cargo run -p xtask -- simdoc --check
```

## Documentation Lanes

`cargo run -p xtask -- simdoc` builds the public documentation lanes:

- API docs: `target/doc/`
- Agent cards: `docs/agents/cards.jsonl` and `docs/agents/card-index.json`
- Human docs: `docs/humans/`
- Diagrams: `docs/diagrams/src/` and `docs/diagrams/generated/`

The same command writes split contract files under `docs/generated/`.
