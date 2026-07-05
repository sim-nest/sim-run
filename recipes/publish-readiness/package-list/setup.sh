test -n "$SIM_META_WORKSPACE_MANIFEST"
cargo package --manifest-path "$SIM_META_WORKSPACE_MANIFEST" -p sim-run-core --allow-dirty --list
cargo package --manifest-path "$SIM_META_WORKSPACE_MANIFEST" -p sim-run --allow-dirty --list
