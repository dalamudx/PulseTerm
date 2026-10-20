#!/bin/sh
# Build the Windows echo-latency kit: native core test, MinGW cross-compile, then zip.
# Needs a MinGW-w64 compiler (CC_WIN) and the installed ui/node_modules xterm packages.
set -eu
out=${1:?usage: build-kit.sh OUTPUT_DIR}
src=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$src/../.." && pwd)
cc_win=${CC_WIN:-x86_64-w64-mingw32-gcc}
xterm="$root/ui/node_modules/@xterm"
grep -q '"version": "6.0.0"' "$xterm/xterm/package.json"
grep -q '"version": "0.19.0"' "$xterm/addon-webgl/package.json"
kit="$out/pulseterm-echo-latency"
rm -rf -- "$kit" "$out/pulseterm-echo-latency.zip"
mkdir -p "$kit/xterm"

${CC:-cc} -std=c11 -D_POSIX_C_SOURCE=200809L -Wall -Wextra -Werror "$src/core_test.c" -o "$out/core-test"
"$out/core-test"
rm -f -- "$out/core-test"

flags="-std=gnu11 -O2 -Wall -Wextra -Werror -static -s"
$cc_win $flags "$src/echo-latency.c" -o "$kit/echo-latency.exe" -ld3d11 -ldxgi
$cc_win $flags "$src/echo-console.c" -o "$kit/echo-console.exe"

cp "$src/echo.html" "$src/run.ps1" "$src/README.md" "$kit/"
cp -L "$xterm/xterm/lib/xterm.js" "$xterm/xterm/css/xterm.css" "$xterm/addon-webgl/lib/addon-webgl.js" "$kit/xterm/"
cp -L "$xterm/xterm/LICENSE" "$kit/xterm/LICENSE-xterm"
cp -L "$xterm/addon-webgl/LICENSE" "$kit/xterm/LICENSE-addon-webgl"
(cd "$out" && zip -qrX pulseterm-echo-latency.zip pulseterm-echo-latency && sha256sum pulseterm-echo-latency.zip)
