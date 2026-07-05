# sim-cli

`sim-cli` provides the `sim` binary. The binary delegates command handling to
`sim-run-core` and keeps its entrypoint thin.

This is a pre-publish bootloader frame: it bakes in no codec. By default it
never fetches code over the network and boots only libraries supplied via
`--load` (an artifact source) or already present in the local cache, so with no
source it reports `no codec '<name>' available`. Feature builds compose loader
mechanisms such as `dynamic-native`, `registry`, and `wasm`; behavior lives in
loadable libraries, not in the frame.
