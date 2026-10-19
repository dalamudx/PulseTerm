//! Fixed, connection-owned background output for the native SSH scroll fixture.
use std::{path::Path, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use russh::{ChannelId, server};
use tokio::{task::JoinHandle, time::Instant};

pub(super) struct Output(JoinHandle<()>);

impl Drop for Output {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct Exited;
impl Drop for Exited {
    fn drop(&mut self) {
        eprintln!("fixture.ssh_scope_async_exited");
    }
}

async fn trigger(directory: &Path, index: usize, deadline: Instant) -> Result<()> {
    tokio::time::timeout_at(deadline, async {
        loop {
            match tokio::fs::symlink_metadata(directory.join(format!("emit-{index}"))).await {
                Ok(metadata) if metadata.is_file() => return Ok(()),
                Ok(_) => bail!("scope.output_trigger_not_regular_file"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .context("scope.output_trigger_deadline")?
}

impl Output {
    pub(super) fn start(handle: server::Handle, channel: ChannelId, directory: PathBuf) -> Self {
        Self(tokio::spawn(async move {
            let _exited = Exited;
            eprintln!("fixture.ssh_scope_async_started");
            let result: Result<()> = async {
                let deadline = Instant::now() + Duration::from_secs(120);
                for index in 1..=5 {
                    trigger(&directory, index, deadline).await?;
                    let output = format!("SSH_SCROLL_ASYNC_{index}\r\n").repeat(12) + "\x07";
                    tokio::time::timeout(
                        Duration::from_secs(2),
                        handle.data(channel, output.into_bytes()),
                    )
                    .await
                    .context("scope.output_send_deadline")?
                    .map_err(|_| anyhow::anyhow!("scope.output_channel_closed"))?;
                    eprintln!("fixture.ssh_scope_async_sent.{index}");
                }
                Ok(())
            }
            .await;
            if result.is_err() {
                eprintln!("fixture.ssh_scope_async_failed");
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn only_the_requested_regular_trigger_can_release_output() -> Result<()> {
        let directory = tempfile::tempdir()?;
        std::fs::write(directory.path().join("emit-2"), [])?;
        assert!(
            trigger(
                directory.path(),
                1,
                Instant::now() + Duration::from_millis(30)
            )
            .await
            .is_err()
        );
        trigger(directory.path(), 2, Instant::now() + Duration::from_secs(1)).await?;
        std::fs::create_dir(directory.path().join("emit-3"))?;
        assert!(
            trigger(directory.path(), 3, Instant::now() + Duration::from_secs(1))
                .await
                .is_err()
        );
        Ok(())
    }

    #[tokio::test]
    async fn dropping_the_connection_owner_cancels_a_waiting_output_task() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let (started, ready) = tokio::sync::oneshot::channel();
        let path = directory.path().to_owned();
        let task = tokio::spawn(async move {
            let _ = started.send(());
            assert!(
                trigger(&path, 1, Instant::now() + Duration::from_secs(120))
                    .await
                    .is_ok()
            );
        });
        let abort = task.abort_handle();
        let owner = Output(task);
        ready.await?;
        drop(owner);
        tokio::time::timeout(Duration::from_secs(1), async {
            while !abort.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        Ok(())
    }
}
