# sim-lib-hotload

In one line: Structured offline sandboxed native library builds for SIM.

## What it gives you

Build native SIM extensions without exposing a shell or mutable output path. `sim-lib-hotload` validates a sealed Cargo package, requires offline locked execution under proven sandbox controls, selects the single Cargo-reported `cdylib`, and publishes verified bytes by content identity. Before activation, candidates are re-verified, inspected through the loader membrane, checked against the managed generation and live dependency relation, and exercised with bounded conformance tests in an isolated shadow context. Operate the whole lifecycle without receiving shell or filesystem authority. Five loadable, Card-described functions share one typed record contract across Rust and. The contract keeps inputs, outputs, limits, and refusal cases explicit, so callers can compose the capability without acquiring unrelated host, transport, or product authority. Stable records make the result suitable for tests, inspection, and deterministic integration.

## Why you will be glad

- The public contract makes supported behavior, limits, and typed failures visible before integration.
- One owning crate prevents neighboring libraries from growing competing copies of the same policy.
- Deterministic records and checked tests keep adapters reviewable when implementations evolve.

## Where it fits

Within SIM, sim-lib-hotload owns only the focused contract described above. Adjacent runtime libraries, platform adapters, codecs, and user surfaces can build around it while retaining their own policy. That boundary keeps the kernel small, avoids competing implementations, and lets this capability evolve without forcing unrelated components to change.
