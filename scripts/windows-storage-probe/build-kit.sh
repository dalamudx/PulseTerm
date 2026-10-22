#!/bin/sh
# Cross-build existing Rust tests; the toolchain version remains managed by mise.
set -eu
out=${1:?usage: build-kit.sh OUTPUT_DIR}
root=$(cd "$(dirname "$0")/../.." && pwd)
scratch=$(mktemp -d /data/temp/windows-storage-build.XXXXXX)
trap 'rm -rf -- "$scratch"' EXIT HUP INT TERM
export TMPDIR="$scratch" TMP="$scratch" TEMP="$scratch" CARGO_TARGET_DIR="$scratch/target"
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-static -C debuginfo=0"
cc_win=${CC_WIN:-x86_64-w64-mingw32-gcc}
compiler_bin=$(dirname "$(command -v "$cc_win")")
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$cc_win"
cargo test --manifest-path "$root/Cargo.toml" -p pulse-platform --lib --target x86_64-pc-windows-gnu --no-run --locked --message-format=json > "$scratch/artifacts.jsonl"
kit="$scratch/windows-storage-probe"
node "$root/scripts/windows-storage-probe/assemble.mjs" "$root" "$scratch/artifacts.jsonl" "$kit" "$compiler_bin/../share/doc"
(cd "$kit" && sha256sum -c SHA256SUMS)
(cd "$scratch" && zip -qrX windows-storage-probe-v3.zip windows-storage-probe)
mkdir -p "$out"
cp -- "$scratch/windows-storage-probe-v3.zip" "$out/windows-storage-probe-v3.zip"
sha256sum "$out/windows-storage-probe-v3.zip"
