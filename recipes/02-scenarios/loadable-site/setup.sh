test -n "$SIM_META_WORKSPACE_MANIFEST"
cargo test --manifest-path "$SIM_META_WORKSPACE_MANIFEST" -p sim-lib-agent loaded_site_resolves_through_model_at
