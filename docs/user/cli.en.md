# PulseTerm command-line guide

[简体中文](cli.md)

This is a development preview, pending cross-platform Alpha acceptance. This guide describes the implemented `pulse` interface. Linux has scoped runtime evidence; Windows/macOS warm and cold control are wired, pending native GUI acceptance.

## Offline help and version

```sh
pulse --help
pulse --help=zh-CN
pulse --version
```

`-h` and `-V` are aliases for English help and version. Use each option on its own. They do not read control tokens, contact a workspace or start a GUI. The version is the binary's package version; different builds can share it, so retain any supplied build identifier for problem reports.

In PowerShell, run `.\pulse.exe --help` from the program directory. The CLI requires an explicit `workspace-cli` build; ordinary workspaces and local validation kits do not automatically include it. SSH actions also require an SSH-enabled workspace. Use the CLI and GUI companion from the same build.

## Discover and start local Profiles

```sh
pulse profiles
```

Example output:

```json
{"version":1,"profiles":["bash","sh"]}
```

Use an exact string from your own output as `PROFILE`. Automatic identities include `bash`, `zsh`, `fish`, `sh`, `pwsh`, `powershell`, `cmd` and `wsl`. Only discovered, saved and visible entries are returned; custom saved entries use stable IDs. `pwsh` and `powershell` identify different shells.

On Linux with a discovered `sh` Profile:

```sh
pulse new-tab sh
pulse split -v sh
pulse split-pane -h sh
```

On Windows with a discovered Windows PowerShell Profile:

```powershell
.\pulse.exe new-tab powershell
.\pulse.exe split -v powershell
```

`-v` places panes side by side; `-h` places them above and below each other. Omit PROFILE to use the saved default. Missing, hidden, deleted or unavailable defaults fail without trying another shell. `open PROFILE` is an alias for `new-tab PROFILE`. A Profile is a saved identity, not a program path. Its executable, arguments, working directory and environment come from saved, approved GUI settings.

Approve new terminals in the original GUI window. The CLI waits for presentation before returning `session N`. Use `focus-tab N` to select that session within the window that owns it.

## Start in the current directory

```sh
pulse .
pulse -w workspace .
```

Uses the saved default Profile and waits for approval in the original window, showing both the identity and CLI working directory. Missing, hidden, deleted or unavailable defaults fail without selecting another shell. The directory applies only to this launch; saved Profile settings remain unchanged. Missing or inaccessible directories fail without falling back to the home directory.

Use this form alone, without another path or an action chain. The directory must have a lossless Unicode representation, fit within 4096 UTF-8 bytes and contain no control or hidden direction characters. Windows accepts local drive paths only. Cold startup opens an empty workspace, then requests approval over the same authenticated connection; the path never enters GUI startup arguments. Cancellation, navigation or closing the original page revokes the pending approval.

## Choose a window

```sh
pulse windows
pulse -w workspace focus
pulse -w workspace new-tab sh
```

`windows` returns published labels such as `workspace` and `workspace-…`, not window titles or session numbers. This global list does not accept `-w`. When multiple windows exist, other actions require an exact `-w WINDOW`; copy the full label from the actual list.

`pulse new-window` requires a running GUI and approval in the original window. It creates an empty workspace without copying another window's sessions or Vault unlock state.

## Connect to a saved SSH entry

```sh
pulse open --host my-server
```

The entry name must exactly match a saved record. Quote names containing spaces according to your shell's rules. `open my-server` resolves a local Profile, not an SSH entry.

The explicit ID interface is:

```text
pulse ssh HOST_ID --identity IDENTITY_ID [AUTH]
```

Use IDs from existing saved Host and Identity records. Raw addresses, username parameters, passwords, key bytes and arbitrary programs are not accepted. Explicit authentication options are `--interactive`, `--key-interactive`, `--agent-interactive` and `--password-interactive`. They must match the saved identity and the GUI's supported path; failure does not switch methods automatically.

Use the GUI to verify the actual target and first or changed fingerprint, unlock the Vault and approve authentication. Enter MFA answers in that original GUI. Saving configuration or connecting to the control channel does not authorize network access or credential use. Cancelling approval does not automatically connect.

## Chain actions

```sh
pulse profiles . focus
pulse profiles ';' focus
pulse -w workspace profiles . -w workspace focus
```

`.` and `;` separate complete actions. Quote the semicolon so your shell does not execute it. Used alone, `pulse .` starts a terminal in the current directory; that form cannot be chained.

A chain accepts at most 8 actions and 64 arguments. All syntax is checked before the first action starts. Specify `-w` for each action that needs it; selection is not inherited by later steps. A cold first action and its continuation use the same connection.

Each step waits for a confirmed result. Rejection, timeout, disconnection or an unconfirmed result stops later steps. Completed sessions remain. There is no rollback, reconnection or automatic retry of uncertain actions. Check the GUI's actual state before deciding what to do next.

## Cold startup and troubleshooting

macOS cold startup requires `pulse` and `pulseterm-workspace` in the same trusted directory; it does not search `.app` bundles, PATH or other installations, or invoke a shell. The companion must be a regular executable with one link, owned by the account or root, without setid; its permissions and ancestor directories must pass write-permission checks. If this layout is unavailable, open the matching workspace normally and use warm control. Permissions are never repaired. A secondary GUI cannot claim an occupied endpoint or forward startup arguments.
Supported first actions can start the fixed `pulseterm-workspace` companion beside the CLI (`.exe` on Windows), then wait for original-page approval. `windows` and `new-window` require a running GUI. Cold startup never searches for arbitrary programs. Windows rejects startup-action forwarding from secondary GUI instances; concurrent cold launches can fail.

| Symptom | What to do |
|---|---|
| Invalid action or arguments | Check offline help. Syntax errors return 2 without executing the earlier part of a chain. |
| Profile unavailable | Check `profiles` and use the GUI to discover, save or show it. Do not substitute a program path. |
| Ambiguous window | Obtain its full label with `windows` and put `-w` before that action. |
| Missing GUI companion | Use the companion from the same build, or open the GUI normally. Do not rename another program as a substitute. |
| Unsafe, stale or unauthenticated endpoint | No automatic repair, token deletion or cold fallback occurs. Retain the error and actual process state. |
| Timeout or unconfirmed action | Exit code 1. Check whether the GUI completed it before creating or connecting again. |

A normally confirmed result returns 0; a failed or unconfirmed action returns 1; invalid syntax returns 2. Help or version output does not establish platform acceptance or release readiness.

For a problem report, include the version, supplied build identifier, operating system, command structure, failure stage, exit code and whether the GUI remains running. Redact sensitive entry and host names. Do not attach passwords, private keys, MFA answers or Vault databases.
