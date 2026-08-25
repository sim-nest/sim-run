# sim-lib-hotload

`sim-lib-hotload` builds one native SIM library from a sealed source mount using
the shared sandbox contract. Requests are structured, Cargo invocation is fixed,
and successful bytes are copied into a preopened content-addressed artifact mount
before any candidate is returned.

Admission re-reads those immutable bytes, inspects the manifest only through the
selected loader port, checks replacement compatibility and authoritative loaded
dependents, then realizes the candidate in a fresh shadow context containing
only its dependency receipts. Every candidate-registered conformance test must
pass within explicit evidence bounds before a content-identified admission
receipt is returned.

The crate owns portable policy only. Host process realization, filesystem mounts,
native loading, journaling, and activation remain in their existing owners.

## Loadable operations

`HotloadLib` exports `hotload/build`, `hotload/admit`, `hotload/activate`,
`hotload/status`, and `hotload/history`, plus named argument/result Shapes and
`hotload/operation-cards`. Build and activation use distinct `hotload/build`
and `hotload/activate` capabilities; admission uses `hotload/admit`, while the
two browse operations use `hotload/inspect`.

Every call accepts one `HotloadRecord` and returns a typed `HotloadRecord`.
These records encode in data position as `#(hotload/Record KIND FIELDS)`, so a
Lisp caller and a Rust caller cross exactly the same validation and dispatch
path. Records contain stable content and generation identities, compatibility
differences, sandbox bounds, reachability, refusal details, and journal
evidence. They never contain host paths, argv, raw handles, loader/provider
objects, or a general execution capability.

The host composes `HotloadPort` from the existing immutable builder, admission,
activation, loader, storage, and journal boundaries. That membrane is the only
place effects can occur; the loadable surface remains portable data and policy.
