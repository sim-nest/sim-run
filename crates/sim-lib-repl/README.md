# sim-lib-repl

sim-lib-repl is the loadable command-line REPL library for SIM. It exports
`cli/main/repl` for the bootloader handoff and keeps the read-eval-print core in
the public `eval_line` helper so hosts can test it in-process.

The library expects the active runtime context to provide a codec, number
domains, and evaluator behavior through loaded libraries. It does not install
that eval stack itself.
