#!/bin/sh
# Linux cross-build; temporary SDK extraction stays below /data/temp.
set -eu
out=${1:?usage: build-kit.sh OUTPUT_DIR}
src=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$src/../.." && pwd)
cc_win=${CC_WIN:-x86_64-w64-mingw32-gcc}
cxx_win=${CXX_WIN:-x86_64-w64-mingw32-g++}
package=${WEBVIEW2_PACKAGE:?Set WEBVIEW2_PACKAGE to Microsoft.Web.WebView2 1.0.4258.31 nupkg}
printf '%s  %s\n' 56f7f4b8bf9aee4b8efefbbdd4f67d5f74ebd1b100ed0806da71bf76af481aa9 "$package" | sha256sum -c -
scratch=$(mktemp -d /data/temp/windows-echo-build.XXXXXX)
trap 'rm -rf -- "$scratch"' EXIT HUP INT TERM
export TMPDIR="$scratch" TMP="$scratch" TEMP="$scratch"
mkdir -p "$scratch/sdk"
unzip -p "$package" build/native/include/WebView2.h > "$scratch/sdk/WebView2.h"
xterm="$root/ui/node_modules/@xterm"
grep -q '"version": "6.0.0"' "$xterm/xterm/package.json"
grep -q '"version": "0.19.0"' "$xterm/addon-webgl/package.json"
kit="$out/pulseterm-echo-latency"
mkdir -p "$kit/xterm" "$kit/third-party"
rm -f -- "$out/pulseterm-echo-latency.zip"
${CC:-cc} -std=c11 -D_POSIX_C_SOURCE=200809L -Wall -Wextra -Werror "$src/core_test.c" -o "$scratch/core-test"
"$scratch/core-test"
${CXX:-c++} -std=c++17 -Wall -Wextra -Werror "$src/navigation_test.cpp" -o "$scratch/navigation-test"
"$scratch/navigation-test"
flags="-std=gnu11 -O2 -Wall -Wextra -Werror -static -s"
$cc_win $flags "$src/echo-latency.c" -o "$kit/echo-latency.exe" -ld3d11 -ldxgi
$cc_win $flags "$src/echo-console.c" -o "$kit/echo-console.exe"
$cxx_win -std=c++17 -O2 -Wall -Wextra -Werror -static -s -municode -mwindows -DUNICODE -D_UNICODE \
    -I "$src/mingw" -isystem "$scratch/sdk" "$src/webview-host.cpp" -o "$kit/echo-webview2.exe" -lole32 -lshell32 -luuid
unzip -p "$package" runtimes/win-x64/native/WebView2Loader.dll > "$kit/WebView2Loader.dll"
unzip -p "$package" LICENSE.txt > "$kit/third-party/LICENSE-WebView2.txt"
unzip -p "$package" NOTICE.txt > "$kit/third-party/NOTICE-WebView2.txt"
compiler_bin=$(dirname "$(command -v "$cxx_win")")
compiler_docs="$compiler_bin/../share/doc"
# Static compiler runtimes retain their complete distribution notices and exception text.
cp -L "$compiler_docs/gcc-mingw-w64-base/copyright" "$kit/third-party/NOTICE-GCC-Runtime.txt"
cp -L "$compiler_docs/mingw-w64-common/copyright" "$kit/third-party/NOTICE-MinGW.txt"
cp /usr/share/common-licenses/GPL-3 "$kit/third-party/GPL-3.txt"
printf '%s\n' 'Microsoft.Web.WebView2 1.0.4258.31' \
    'https://api.nuget.org/v3-flatcontainer/microsoft.web.webview2/1.0.4258.31/microsoft.web.webview2.1.0.4258.31.nupkg' \
    'SHA256 56f7f4b8bf9aee4b8efefbbdd4f67d5f74ebd1b100ed0806da71bf76af481aa9' > "$kit/third-party/WebView2-source.txt"
cp "$src/echo.html" "$src/run.ps1" "$src/README.md" "$kit/"
cp -L "$xterm/xterm/lib/xterm.js" "$xterm/xterm/css/xterm.css" "$xterm/addon-webgl/lib/addon-webgl.js" "$kit/xterm/"
cp -L "$xterm/xterm/LICENSE" "$kit/xterm/LICENSE-xterm"
cp -L "$xterm/addon-webgl/LICENSE" "$kit/xterm/LICENSE-addon-webgl"
(cd "$kit" && sha256sum echo-latency.exe echo-console.exe echo-webview2.exe WebView2Loader.dll run.ps1 echo.html > SHA256SUMS)
(cd "$out" && zip -qrX pulseterm-echo-latency.zip pulseterm-echo-latency && sha256sum pulseterm-echo-latency.zip)
