# Browse the hot-generation contract

This checked specimen proves the loadable library advertises all five lifecycle
operations and keeps build authority separate from activation authority. A host
application injects the effectful `HotloadPort`; Lisp and Rust callers use the
same record, Shape, capability, idempotence, and typed-outcome path.
