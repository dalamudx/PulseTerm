//! Static CLI information; no endpoint, filesystem, environment or GUI access.
use std::ffi::OsString;

pub(super) const VERSION: &str = concat!("PulseTerm CLI ", env!("CARGO_PKG_VERSION"), "\n");

const HELP_EN: &str = "\
PulseTerm CLI

Usage: pulse [-w WINDOW] ACTION [ . ACTION | ';' ACTION ... ]
       pulse --help | --help=zh-CN | --version

Actions:
  focus                         Request focus for a workspace window
  profiles                      List available saved Profile identities
  windows                       List published window labels
  new-window                    Create an empty window (running GUI required)
  new-tab PROFILE               Start a saved local Profile; open is an alias
  split -v|-h [PROFILE]           Split side by side (-v) or above/below (-h)
                                split-pane is an alias
  focus-tab SESSION             Select a session owned by the chosen window
  open --host NAME              Resolve an exact saved SSH entry name
  ssh HOST --identity ID [AUTH]  Connect using saved Host and Identity IDs

AUTH: --interactive | --key-interactive | --agent-interactive |
      --password-interactive (SSH build required)

Omit PROFILE for split to use the saved default; unavailable defaults fail.
Use profiles to choose an available identity. No program paths, shell arguments,
addresses, passwords, keys or environment overrides are accepted as launch input.
New sessions, SSH and new windows require approval in the original GUI window.
Pass -w before each action when selecting among multiple windows.
Chains allow at most 8 actions and 64 arguments. Quote ';' in your shell.
A failed step stops the chain; completed sessions remain. There is no rollback
or automatic retry of an uncertain action.
Use pulse [-w WINDOW] . alone to start the saved default in the CLI's current
working directory after GUI approval. No saved settings change; do not chain it.

Examples:
  $ pulse profiles
  $ pulse -w workspace focus
  $ pulse profiles . focus

Linux and opt-in Windows builds support control. macOS requires a running GUI.
Cold startup requires the matching pulseterm-workspace companion beside pulse;
new-window and windows require a running GUI. Unsafe or stale endpoints fail.
Windows/macOS native GUI acceptance is pending. Preview builds are not a release.
Help/version are offline and never read tokens or start a GUI.
Exit codes: 0 confirmed result, 1 failure/unconfirmed action, 2 invalid syntax.
中文帮助: pulse --help=zh-CN
";

const HELP_ZH: &str = "\
PulseTerm 命令行

用法：pulse [-w WINDOW] ACTION [ . ACTION | ';' ACTION ... ]
      pulse --help | --help=zh-CN | --version

动作：
  focus                         请求聚焦工作区窗口
  profiles                      列出可用的已保存 Profile 身份
  windows                       列出已发布的窗口标签
  new-window                    创建空窗口（要求 GUI 已运行）
  new-tab PROFILE               启动已保存的本地 Profile，open 是其别名
  split -v|-h [PROFILE]           左右分屏（-v）或上下分屏（-h）
                                split-pane 是其别名
  focus-tab SESSION             选择目标窗口拥有的会话
  open --host NAME              按精确名称解析已保存的 SSH 入口
  ssh HOST --identity ID [AUTH]  使用已保存 Host 与 Identity ID 连接

AUTH：--interactive | --key-interactive | --agent-interactive |
      --password-interactive（需要含 SSH 的构建）

split 可省略 PROFILE，仅用已保存默认项；未设置或不可用时失败，不选其他 Shell。
先用 profiles 选择可用身份。不接受程序路径、Shell 参数、原始地址、口令、
密钥或环境覆盖作为启动输入。新会话、SSH 和新窗口均需原 GUI 窗口明确批准。
多窗口时，在每个动作前分别指定 -w；窗口标签不授予会话或凭据权限。
命令链最多 8 步、64 个参数；分号须在 Shell 中加引号。
失败立即停止后续步骤，已完成的会话保留；不回滚或自动重试不确定的动作。
单独使用 pulse [-w WINDOW] .，批准后用已保存默认身份在 CLI 当前目录启动。
仅覆盖本次工作目录，不修改保存设置；此形式不参与命令链。

示例：
  $ pulse profiles
  $ pulse -w workspace focus
  $ pulse profiles . focus

Linux 与显式 Windows 构建可使用控制入口；macOS 必须先打开配套 GUI。
冷启动要求同目录下匹配的 pulseterm-workspace 配套程序；
new-window 与 windows 要求 GUI 已运行。不安全或陈旧端点直接失败。
Windows／macOS 原生 GUI 尚待验收，预览构建不代表正式发行。
帮助和版本离线返回，不读取 token 或启动 GUI。
退出码：0 已确认结果，1 失败／未确认动作，2 语法无效。
English help: pulse --help
";

pub(super) fn output(args: &[OsString]) -> Option<&'static str> {
    if args.len() != 1 {
        return None;
    }
    match args[0].to_str()? {
        "--help" | "-h" => Some(HELP_EN),
        "--help=zh-CN" => Some(HELP_ZH),
        "--version" | "-V" => Some(VERSION),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn query(args: &[&str]) -> Option<&'static str> {
        output(&args.iter().map(OsString::from).collect::<Vec<_>>())
    }
    #[test]
    fn exact_standalone_flags_return_both_languages_and_package_version() {
        assert_eq!(query(&["--help"]), query(&["-h"]));
        assert!(query(&["--help"]).is_some_and(|text| text.contains("Exit codes:")));
        assert!(query(&["--help=zh-CN"]).is_some_and(|text| text.contains("退出码")));
        assert_eq!(query(&["--version"]), Some(VERSION));
        assert_eq!(query(&["-V"]), Some(VERSION));
        assert!(VERSION.contains(env!("CARGO_PKG_VERSION")));
    }
    #[test]
    fn information_never_consumes_window_selectors_actions_or_chains() {
        for args in [
            vec![],
            vec!["help"],
            vec!["--help=fr"],
            vec!["--help", "focus"],
            vec!["focus", ".", "--help"],
            vec!["-w", "workspace", "--help"],
            vec!["--help", ";", "new-window"],
            vec!["--version", "--help"],
            vec!["-h", "sh"],
            vec!["split", "-h", "sh"],
        ] {
            assert!(query(&args).is_none());
        }
    }
    #[cfg(unix)]
    #[test]
    fn non_unicode_arguments_never_become_information_flags() {
        use std::os::unix::ffi::OsStringExt;
        assert!(output(&[OsString::from_vec(vec![0xff])]).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn non_unicode_arguments_never_become_information_flags() {
        use std::os::windows::ffi::OsStringExt;
        assert!(output(&[OsString::from_wide(&[0xd800])]).is_none());
    }
}
