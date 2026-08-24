# Immutable native builds

Build native SIM extensions without exposing a shell or mutable output path.
`sim-lib-hotload` validates a sealed Cargo package, requires offline locked
execution under proven sandbox controls, selects the single Cargo-reported
`cdylib`, and publishes verified bytes by content identity.
