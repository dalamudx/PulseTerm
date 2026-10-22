//! Fixed local OpenSSH Agent connection and bounded, revocable pipe-busy retry.
use super::{AgentError, Authorize};
use std::{io, time::Duration};

#[cfg(windows)]
pub(super) async fn connect(current: Authorize) -> Result<super::AgentStream, AgentError> {
    // Tokio defaults to SECURITY_IDENTIFICATION | SECURITY_SQOS_PRESENT: no server impersonation.
    connect_with(
        || {
            tokio::net::windows::named_pipe::ClientOptions::new()
                .open(r"\\.\pipe\openssh-ssh-agent")
        },
        current,
        Duration::from_secs(3),
    )
    .await
}

async fn connect_with<T>(
    mut open: impl FnMut() -> io::Result<T>,
    current: Authorize,
    timeout: Duration,
) -> Result<T, AgentError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if !current() {
            return Err(AgentError::Denied);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AgentError::Unavailable);
        }
        match open() {
            Ok(stream) => {
                if !current() {
                    return Err(AgentError::Denied);
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err(AgentError::Unavailable);
                }
                return Ok(stream);
            }
            // ERROR_PIPE_BUSY is the only retryable open error. Never wait in blocking WaitNamedPipe.
            Err(error) if error.raw_os_error() == Some(231) => {
                tokio::time::sleep_until(
                    (tokio::time::Instant::now() + Duration::from_millis(10)).min(deadline),
                )
                .await;
            }
            Err(_) => return Err(AgentError::Unavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    #[tokio::test]
    async fn denied_before_open_never_touches_native_transport() {
        assert!(matches!(
            connect_with::<()>(
                || panic!("must not open"),
                Arc::new(|| false),
                Duration::from_secs(1)
            )
            .await,
            Err(AgentError::Denied)
        ));
    }

    #[tokio::test]
    async fn busy_retries_then_succeeds_and_other_errors_never_retry() {
        let mut attempts = 0;
        let result = connect_with(
            || {
                attempts += 1;
                if attempts == 1 {
                    Err(io::Error::from_raw_os_error(231))
                } else {
                    Ok(7)
                }
            },
            Arc::new(|| true),
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(result.ok(), Some(7));
        assert_eq!(attempts, 2);
        for code in [2, 5, 53] {
            let mut attempts = 0;
            let result = connect_with::<()>(
                || {
                    attempts += 1;
                    Err(io::Error::from_raw_os_error(code))
                },
                Arc::new(|| true),
                Duration::from_secs(1),
            )
            .await;
            assert!(matches!(result, Err(AgentError::Unavailable)));
            assert_eq!(attempts, 1);
        }
    }

    #[tokio::test]
    async fn revocation_during_busy_retry_or_success_never_publishes_transport() {
        for succeeds in [false, true] {
            let alive = Arc::new(AtomicBool::new(true));
            let flag = alive.clone();
            let grant: Authorize = Arc::new(move || flag.load(Ordering::Acquire));
            let mut attempts = 0;
            let result = connect_with(
                || {
                    attempts += 1;
                    alive.store(false, Ordering::Release);
                    if succeeds {
                        Ok(())
                    } else {
                        Err(io::Error::from_raw_os_error(231))
                    }
                },
                grant,
                Duration::from_secs(1),
            )
            .await;
            assert!(matches!(result, Err(AgentError::Denied)));
            assert_eq!(attempts, 1);
        }
    }

    #[tokio::test]
    async fn busy_has_one_deadline_and_zero_budget_never_opens() {
        let start = tokio::time::Instant::now();
        let result = connect_with::<()>(
            || Err(io::Error::from_raw_os_error(231)),
            Arc::new(|| true),
            Duration::from_millis(15),
        )
        .await;
        assert!(matches!(result, Err(AgentError::Unavailable)));
        assert!(start.elapsed() >= Duration::from_millis(15));
        assert!(matches!(
            connect_with::<()>(
                || panic!("must not open"),
                Arc::new(|| true),
                Duration::ZERO
            )
            .await,
            Err(AgentError::Unavailable)
        ));
    }
}
