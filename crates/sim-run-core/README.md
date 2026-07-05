# sim-run-core

`sim-run-core` provides the command entry API used by the `sim` binary. It
parses boot controls into `CliBoot`, preserves handoff payloads in `Payload`,
projects `CliEnvelope`, and loads the selected boot codec as the first library.
The default codec name is `lisp`, which maps to `codec/lisp`; `--codec NAME`
maps to `codec/NAME`. `LoadReceipt` records whether a loaded library is the
boot codec or an ordinary requested library.

The first non-flag token starts the loaded-library payload; `--` is available
when a caller wants an explicit boundary before payload tokens.

After loading, `LoadSession` selects a loaded library with a resolved function
export under `cli/main`, such as `cli/main/demo`. Explicitly loaded libraries
take precedence over the boot codec. The handoff passes the `CliEnvelope` as a
kernel table value, then maps the returned value to an exit code by truthiness.

`LibSourceSpec` parses command-line source syntax, including
`crates.io:PACKAGE@REQ` sources resolved through the CLI-owned cache. With the
`registry` feature, callers can install a `GitRegistryResolver` so cache misses
fetch from an explicit git registry artifact endpoint and then resolve as local
`path:` sources.
