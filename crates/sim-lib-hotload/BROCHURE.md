# Immutable native builds

Build native SIM extensions without exposing a shell or mutable output path.
`sim-lib-hotload` validates a sealed Cargo package, requires offline locked
execution under proven sandbox controls, selects the single Cargo-reported
`cdylib`, and publishes verified bytes by content identity.

Before activation, candidates are re-verified, inspected through the loader
membrane, checked against the managed generation and live dependency relation,
and exercised with bounded conformance tests in an isolated shadow context.

Operate the whole lifecycle without receiving shell or filesystem authority.
Five loadable, Card-described functions share one typed record contract across
Rust and Lisp. Separate build and activate capabilities let operators delegate
construction without granting live mutation, while status and history explain
generation identity, old-generation reachability, refusals, compatibility,
sandbox limits, and durable journal evidence.
