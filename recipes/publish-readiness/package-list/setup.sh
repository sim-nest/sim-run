test -n "$SIM_META_WORKSPACE_MANIFEST"
cargo package --manifest-path "$SIM_META_WORKSPACE_MANIFEST" -p sim-cli-core --allow-dirty --list
cargo package --manifest-path "$SIM_META_WORKSPACE_MANIFEST" -p sim-cli --allow-dirty --list
