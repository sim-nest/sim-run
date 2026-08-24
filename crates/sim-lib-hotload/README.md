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
