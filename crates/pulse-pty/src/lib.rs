//! Blocking native PTY primitive, with no hidden reader thread or output queue.
//! Callers supervise blocking I/O and keep draining output while closing ConPTY.
#![forbid(unsafe_code)]
pub use portable_pty::ExitStatus;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use pulse_core::TermSize;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;

/// Literal executable and argv, never interpolated into a shell command.
#[derive(Debug, Clone)]
pub struct PtyCommand {
    /// Executable path or name resolved by the native backend.
    pub program: OsString,
    /// Arguments passed verbatim.
    pub args: Vec<OsString>,
    /// None inherits the application's current directory.
    pub cwd: Option<PathBuf>,
    /// Overrides; remaining environment is inherited when building the command.
    pub env: Vec<(OsString, OsString)>,
}
impl PtyCommand {
    /// Explicit command without arguments or overrides.
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: vec![],
            cwd: None,
            env: vec![],
        }
    }
    fn build(&self) -> Result<CommandBuilder, PtyError> {
        if self.program.is_empty() {
            return Err(PtyError::EmptyProgram);
        }
        let mut command = CommandBuilder::new(&self.program);
        command.args(&self.args);
        if let Some(cwd) = &self.cwd {
            command.cwd(cwd);
        }
        command.env("TERM", "xterm-256color");
        for (key, value) in &self.env {
            command.env(key, value);
        }
        Ok(command)
    }
}
/// Backend diagnostics must be redacted before display outside the local app.
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    /// Empty program rejected before OS allocation.
    #[error("PTY executable must not be empty")]
    EmptyProgram,
    /// Native backend failure.
    #[error("PTY backend: {0}")]
    Backend(String),
    /// Read/write/wait failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Competing output readers are not allowed.
    #[error("PTY output reader already taken")]
    ReaderTaken,
    /// Input writer already dropped.
    #[error("PTY input closed")]
    InputClosed,
}
fn size(size: TermSize) -> PtySize {
    PtySize {
        cols: size.columns(),
        rows: size.rows(),
        pixel_width: 0,
        pixel_height: 0,
    }
}
/// Owns the master, writer and directly spawned child. All operations are blocking;
/// do not run on UI/async worker threads. Termination does NOT kill descendants.
/// On Windows the transferred reader must drain concurrently during teardown.
pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Option<Box<dyn Write + Send>>,
    reader: Option<Box<dyn Read + Send>>,
    child: Box<dyn Child + Send + Sync>,
    status: Option<ExitStatus>,
}
impl PtySession {
    /// Spawn literal argv. Parent slave is immediately dropped to permit Unix EOF.
    pub fn spawn(command: &PtyCommand, dimensions: TermSize) -> Result<Self, PtyError> {
        let command = command.build()?;
        let pair = portable_pty::native_pty_system()
            .openpty(size(dimensions))
            .map_err(|e| PtyError::Backend(e.to_string()))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Backend(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::Backend(e.to_string()))?;
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| PtyError::Backend(e.to_string()))?;
        drop(pair.slave);
        Ok(Self {
            master: pair.master,
            writer: Some(writer),
            reader: Some(reader),
            child,
            status: None,
        })
    }
    /// Transfer the single reader to a supervised pump with bounded buffering.
    pub fn take_reader(&mut self) -> Result<Box<dyn Read + Send>, PtyError> {
        self.reader.take().ok_or(PtyError::ReaderTaken)
    }
    /// Write and flush every byte. Can block if the child stops reading.
    pub fn write_input(&mut self, bytes: &[u8]) -> Result<(), PtyError> {
        let writer = self.writer.as_mut().ok_or(PtyError::InputClosed)?;
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }
    /// Drop the writer once. EOF semantics depend on terminal mode and platform.
    pub fn close_input(&mut self) {
        self.writer.take();
    }
    /// Notify the native backend of validated character dimensions.
    pub fn resize(&self, dimensions: TermSize) -> Result<(), PtyError> {
        self.master
            .resize(size(dimensions))
            .map_err(|e| PtyError::Backend(e.to_string()))
    }
    /// Read back native dimensions.
    pub fn dimensions(&self) -> Result<TermSize, PtyError> {
        let s = self
            .master
            .get_size()
            .map_err(|e| PtyError::Backend(e.to_string()))?;
        TermSize::new(s.cols, s.rows).map_err(|e| PtyError::Backend(e.to_string()))
    }
    /// Observe child exit without blocking, separately from trailing-output drain.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, PtyError> {
        if self.status.is_none() {
            self.status = self.child.try_wait()?;
        }
        Ok(self.status.clone())
    }
    /// Blocking direct-child wait; drain output concurrently to avoid deadlock.
    pub fn wait(&mut self) -> Result<ExitStatus, PtyError> {
        if let Some(s) = &self.status {
            return Ok(s.clone());
        }
        let s = self.child.wait()?;
        self.status = Some(s.clone());
        Ok(s)
    }
    /// Kill and reap the direct child. No process-tree or read cancellation promise.
    pub fn terminate(&mut self) -> Result<ExitStatus, PtyError> {
        if let Some(s) = self.try_wait()? {
            return Ok(s);
        }
        self.child.kill()?;
        self.wait()
    }
}
impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn literal_arguments_and_environment_override() {
        let mut request = PtyCommand::new("executable");
        request.args = vec!["space and ; punctuation".into(), "$(not-a-shell)".into()];
        request.env.push(("TERM".into(), "vt100".into()));
        let command = request.build().unwrap();
        assert_eq!(
            command.get_argv(),
            &vec![
                OsString::from("executable"),
                OsString::from("space and ; punctuation"),
                OsString::from("$(not-a-shell)")
            ]
        );
        assert_eq!(command.get_env("TERM"), Some(std::ffi::OsStr::new("vt100")));
    }
    #[test]
    fn empty_executable_rejected_before_allocation() {
        assert!(matches!(
            PtyCommand::new("").build(),
            Err(PtyError::EmptyProgram)
        ));
    }
    #[test]
    fn dimensions_in_character_order() {
        let s = size(TermSize::new(91, 37).unwrap());
        assert_eq!(
            (s.cols, s.rows, s.pixel_width, s.pixel_height),
            (91, 37, 0, 0)
        );
    }
}
