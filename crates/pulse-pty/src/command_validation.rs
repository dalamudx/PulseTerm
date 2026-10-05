use crate::{MAX_COMMAND_ARGS, MAX_COMMAND_BYTES, MAX_COMMAND_ENV, PtyCommand, PtyError};
use std::ffi::OsStr;
pub(super) fn validate(command: &PtyCommand) -> Result<(), PtyError> {
    if command.program.is_empty() {
        return Err(PtyError::EmptyProgram);
    }
    if command.args.len() > MAX_COMMAND_ARGS
        || command.env.len().saturating_add(command.env_remove.len()) > MAX_COMMAND_ENV
    {
        return Err(PtyError::CommandLimit);
    }
    let mut total = 0usize;
    let mut admit = |value: &OsStr| -> Result<(), PtyError> {
        let bytes = value.as_encoded_bytes();
        if bytes.contains(&0) {
            return Err(PtyError::InvalidCommand);
        }
        total = total
            .checked_add(bytes.len())
            .and_then(|v| v.checked_add(1))
            .ok_or(PtyError::CommandLimit)?;
        if total > MAX_COMMAND_BYTES {
            return Err(PtyError::CommandLimit);
        }
        Ok(())
    };
    admit(&command.program)?;
    for argument in &command.args {
        admit(argument)?;
    }
    if let Some(cwd) = &command.cwd {
        if cwd.as_os_str().is_empty() {
            return Err(PtyError::InvalidCommand);
        }
        admit(cwd.as_os_str())?;
    }
    for key in &command.env_remove {
        if key.is_empty() || key.as_encoded_bytes().contains(&b'=') {
            return Err(PtyError::InvalidCommand);
        }
        admit(key)?;
    }
    for (key, value) in &command.env {
        if key.is_empty() || key.as_encoded_bytes().contains(&b'=') {
            return Err(PtyError::InvalidCommand);
        }
        admit(key)?;
        admit(value)?;
    }
    Ok(())
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::PtySession;
    use pulse_core::TermSize;
    #[test]
    fn nul_fields_and_invalid_environment_fail_before_pty_allocation() {
        for field in 0..7 {
            let mut c = PtyCommand::new("missing-private-executable");
            match field {
                0 => c.program = "secret\0program".into(),
                1 => c.args.push("secret\0argument".into()),
                2 => c.cwd = Some("secret\0cwd".into()),
                3 => c.env.push(("secret\0key".into(), "value".into())),
                4 => c.env.push(("KEY".into(), "secret\0value".into())),
                5 => c.env.push(("".into(), "value".into())),
                _ => c.env.push(("KEY=VALUE".into(), "value".into())),
            }
            assert!(matches!(
                PtySession::spawn(&c, TermSize::new(80, 24).unwrap()),
                Err(PtyError::InvalidCommand)
            ));
            assert_eq!(
                validate(&c).unwrap_err().to_string(),
                "PTY command contains invalid fields"
            );
        }
    }
    #[test]
    fn item_limits_and_total_encoded_bytes_have_exact_edges() {
        let mut c = PtyCommand::new("x");
        c.args = vec!["".into(); MAX_COMMAND_ARGS];
        assert!(validate(&c).is_ok());
        c.args.push("".into());
        assert!(matches!(validate(&c), Err(PtyError::CommandLimit)));
        c.args.clear();
        c.env = (0..MAX_COMMAND_ENV)
            .map(|i| (format!("K{i}").into(), "".into()))
            .collect();
        assert!(validate(&c).is_ok());
        c.env.push(("OVER".into(), "".into()));
        assert!(matches!(validate(&c), Err(PtyError::CommandLimit)));
        c.env.clear();
        c.args = vec!["x".repeat(MAX_COMMAND_BYTES - 3).into()];
        assert!(validate(&c).is_ok());
        c.args[0].push("x");
        assert!(matches!(validate(&c), Err(PtyError::CommandLimit)));
    }
    #[test]
    fn empty_argument_and_duplicate_environment_overrides_remain_literal() {
        let mut c = PtyCommand::new("x");
        c.args = vec!["".into(), " ; $(not-a-shell) 中文🌍 ".into()];
        c.env = vec![
            ("TERM".into(), "vt100".into()),
            ("TERM".into(), "xterm".into()),
        ];
        assert!(validate(&c).is_ok());
        assert_eq!(c.build().unwrap().get_argv()[1], OsStr::new(""));
        assert_eq!(
            c.build().unwrap().get_env("TERM"),
            Some(OsStr::new("xterm"))
        );
        c.cwd = Some("".into());
        assert!(matches!(validate(&c), Err(PtyError::InvalidCommand)));
    }
    #[test]
    fn command_debug_redacts_program_arguments_directory_and_environment() {
        let mut c = PtyCommand::new("secret-program");
        c.args.push("secret-argument".into());
        c.cwd = Some("secret-directory".into());
        c.env.push(("SECRET_KEY".into(), "secret-password".into()));
        let debug = format!("{c:?}");
        for secret in [
            "secret-program",
            "secret-argument",
            "secret-directory",
            "SECRET_KEY",
            "secret-password",
        ] {
            assert!(!debug.contains(secret));
        }
        assert!(debug.contains("argument_count: 1"));
        assert!(debug.contains("environment_override_count: 1"));
    }
}
