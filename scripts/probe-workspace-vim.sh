#!/bin/sh
# Real Vim in a private backend-discovered Profile; never load user files.
set -eu
umask 077
[ -x /usr/bin/vim ] || { echo 'workspace.vim.unsupported_missing_vim' >&2; exit 1; }
mkdir -p /data/temp target/probes
fixture=$(mktemp -d /data/temp/pt-workspace-vim.XXXXXX)
trap 'rm -rf "$fixture"; test ! -e "$fixture"' EXIT
export TMPDIR="$fixture" TMP="$fixture" TEMP="$fixture" PYTHONDONTWRITEBYTECODE=1
export XDG_DATA_HOME="$fixture/data" XDG_CONFIG_HOME="$fixture/config" XDG_CACHE_HOME="$fixture/cache"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$fixture/bin"
mkdir -m 700 "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview"
printf '[terminal]\nfont_size = 13\n' > "$XDG_CONFIG_HOME/dev.pulseterm.workspace-preview/settings.toml"
cat > "$fixture/bin/sh" <<'SHELL'
#!/bin/sh
printf '%s\n' "$$" > "$PULSETERM_VIM_PID"
exec /usr/bin/vim --noplugin -N -u "$PULSETERM_VIM_RC" -U NONE -i NONE -n "$PULSETERM_VIM_DOCUMENT"
SHELL
chmod 700 "$fixture/bin/sh"
cat > "$fixture/vimrc" <<'VIM'
set nocompatible encoding=utf-8 nomodeline noswapfile noundofile nobackup nowritebackup
set autoindent smartindent expandtab shiftwidth=4
set mouse= laststatus=2
let g:pulse_count = 0
set statusline=VIM\ FIXTURE\ %{g:pulse_count}\ %m
function! PulseSnapshot()
  let g:pulse_count += 1
  let receipt = {'count': g:pulse_count, 'lines': getline(1, '$'), 'mode': mode(1), 'modified': &modified, 'bracketed_enabled': &t_BE !=# '', 'mouse': &mouse}
  call writefile([json_encode(receipt)], $PULSETERM_VIM_STATE)
  redrawstatus
  return ''
endfunction
nnoremap <expr> <F6> PulseSnapshot()
inoremap <expr> <F6> PulseSnapshot()
autocmd VimEnter * call PulseSnapshot()
VIM
: > "$fixture/document.txt"
export PULSETERM_VIM_RC="$fixture/vimrc" PULSETERM_VIM_DOCUMENT="$fixture/document.txt"
export PULSETERM_VIM_PID="$fixture/native.pid" PULSETERM_VIM_STATE="$fixture/state.json"
export LC_ALL=C.UTF-8
PATH="$fixture/bin" SHELL="$fixture/bin/sh" /usr/bin/python3 scripts/probe-workspace-vim.py "$fixture"
