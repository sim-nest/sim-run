# sim-lib-index

sim-lib-index is the loadable SIM Index exploration library for the `sim`
bootloader. It exports `cli/main/index`, decodes the embedded public SIM Index
snapshot through `codec/index`, and answers list, show, find, trace, and examples
queries from the same graph used by generated public documentation.

Typical command forms:

```bash
sim index list features
sim index find codec --audience code --json
sim index show feature/sim-run/repl
sim index trace feature/sim-run/bootloader
sim index examples feature/sim-run/repl
```

The text output is stable for terminal use. Passing `--json` returns structured
rows for agents.
