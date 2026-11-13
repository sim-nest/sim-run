# sim-run

`sim-run` provides the `sim` binary. The binary delegates command handling to
`sim-run-core` and keeps its entrypoint thin.

It is a bootloader frame: it bakes in no codec. By default it
never fetches code over the network and boots only libraries supplied via
`--load` (an artifact source) or already present in the local cache, so with no
source it reports `no codec '<name>' available`. Feature builds compose loader
mechanisms such as `dynamic-native`, `registry`, and `wasm`; behavior lives in
loadable libraries, not in the frame.

The standard frame selects `sim-lib-world` for the `world` and `prove`
verbs. World `project`, `diff`, and `why` consume caller-supplied facts and
return stable values without observation or mutation authority. Prove plans,
runs, and reads proof through the same library. The frame contributes the
host factory and the verb source.
