set -eu

test -n "${SIM_META_WORKSPACE_MANIFEST:-}"

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../../.." && pwd)
REPO_MANIFEST="$REPO_ROOT/Cargo.toml"

cargo metadata --manifest-path "$REPO_MANIFEST" --format-version=1 --no-deps |
  python3 -c '
import json
import sys

metadata = json.load(sys.stdin)
packages = {package["id"]: package for package in metadata["packages"]}
for package_id in metadata["workspace_members"]:
    package = packages[package_id]
    publish = package.get("publish")
    if publish is not None and len(publish) == 0:
        continue
    print(package["name"])
' |
  while IFS= read -r package; do
    cargo package --manifest-path "$SIM_META_WORKSPACE_MANIFEST" -p "$package" --allow-dirty --list
  done
