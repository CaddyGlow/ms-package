#!/usr/bin/env bash
# Run authoring in a Worker and reopen through the archive-rs browser reader.
set -euo pipefail
if [[ $# -ne 2 ]]; then
  echo "usage: $0 ARCHIVE_RS_CHECKOUT ARCHIVE_WASM_JS_DIRECTORY" >&2
  exit 2
fi
archive_checkout=$(realpath "$1")
archive_bindings=$(realpath "$2")
package_checkout=$(cd "$(dirname "$0")/.." && pwd)
authoring_root=$(mktemp -d /tmp/ms-package-authoring-worker.XXXXXX)
server_pid=
cleanup() {
  if [[ -n "$server_pid" ]]; then kill "$server_pid" 2>/dev/null || true; fi
  # Retain generated artifacts and logs for diagnosis and provenance.
  echo "authoring Worker evidence: $authoring_root" >&2
}
trap cleanup EXIT
cd "$package_checkout"
cargo build --manifest-path tests/browser-authoring/Cargo.toml --target wasm32-unknown-unknown --locked
target_root=$(cargo metadata --manifest-path tests/browser-authoring/Cargo.toml --no-deps --format-version 1 --locked | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
wasm-bindgen "$target_root/wasm32-unknown-unknown/debug/ms_package_authoring_worker_check.wasm" --target web --out-dir "$authoring_root/authoring"
cargo run --manifest-path tests/browser-authoring/Cargo.toml --example native --locked -- "$authoring_root/native.msix"
cp tests/browser-authoring/browser.html tests/browser-authoring/worker.js "$authoring_root/"
cd "$archive_checkout"
node scripts/archive-browser-server.mjs "$authoring_root" 8796 "$archive_bindings" >"$authoring_root/server.log" 2>&1 &
server_pid=$!
node scripts/check-archive-browser.mjs http://127.0.0.1:8796 | tee "$authoring_root/result.json"
