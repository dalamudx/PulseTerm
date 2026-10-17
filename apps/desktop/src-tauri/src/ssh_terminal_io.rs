//! Shared credential-free SSH terminal I/O. Call only after bounded authentication and setup.
use pulse_app::{
    protocol_session_owner::ProtocolSessionError, vault_owner::AuthenticatedProtocolParts,
};
use pulse_core::TermSize;
use pulse_ssh::peer::{NativeIoCompletion, PinnedTransport, VerifiedPeer};
use pulse_term::session::{self, ChannelTransport, DriverEndpoint, TransportWrite};
use russh::{Channel, ChannelMsg, client};

pub(crate) struct TerminalConfig {
    pub size: TermSize,
    pub history_rows: usize,
    pub palette: pulse_term::QueryPalette,
    pub history_limit: usize,
}

pub(crate) fn authenticated_parts(
    channel: Channel<client::Msg>,
    transport: PinnedTransport,
    completion: NativeIoCompletion,
    peer: VerifiedPeer,
    initial: Vec<Vec<u8>>,
    config: TerminalConfig,
) -> AuthenticatedProtocolParts {
    let TerminalConfig {
        size,
        history_rows,
        palette,
        history_limit,
    } = config;
    let (endpoint, driver) = ChannelTransport::bounded_driver_with_resize_basis(
        session::ResizeBasis::SshWindowRequestAdmitted,
    );
    let (handle, actor) = session::spawn_session_with_history_limit(
        size,
        history_rows,
        endpoint,
        palette,
        history_limit,
    );
    AuthenticatedProtocolParts::new(handle, async move {
        let DriverEndpoint {
            output,
            mut input,
            mut stop,
            failed,
        } = driver;
        let (mut read, write) = channel.split();
        let reader_peer = peer.clone();
        let writer_failed = failed.clone();
        let mut reader = tokio::spawn(async move {
            let result = async {
                for bytes in initial {
                    output.send(bytes).await.map_err(|_| ())?;
                }
                let mut eof = false;
                let mut close = false;
                while let Some(message) = read.wait().await {
                    if !reader_peer.is_current() {
                        return Err(());
                    }
                    match message {
                        ChannelMsg::Data { data } => {
                            for chunk in data.chunks(8192) {
                                output.send(chunk.to_vec()).await.map_err(|_| ())?;
                            }
                        }
                        ChannelMsg::Eof => eof = true,
                        ChannelMsg::Close => {
                            close = true;
                            break;
                        }
                        ChannelMsg::ExitStatus { .. } => {}
                        _ => return Err(()),
                    }
                }
                if eof && close { Ok(()) } else { Err(()) }
            }
            .await;
            if result.is_err() {
                failed.send_replace(true);
            }
            drop(output);
            result
        });
        let mut writer = tokio::spawn(async move {
            let result = async {
                while let Some(command) = input.recv().await {
                    if !peer.is_current() {
                        return Err(());
                    }
                    match command {
                        TransportWrite::Input(bytes) | TransportWrite::Reply(bytes) => {
                            write.data(&bytes[..]).await.map_err(|_| ())?
                        }
                        TransportWrite::Resize(size, acknowledgement) => {
                            // RFC 4254 window-change has no reply: this acknowledges SDK admission only.
                            let result = write
                                .window_change(
                                    u32::from(size.columns()),
                                    u32::from(size.rows()),
                                    0,
                                    0,
                                )
                                .await
                                .map_err(|_| session::SessionError::Transport);
                            let failed = result.is_err();
                            let _ = acknowledgement.send(result);
                            if failed {
                                return Err(());
                            }
                        }
                    }
                }
                Ok::<_, ()>(())
            }
            .await;
            if result.is_err() {
                writer_failed.send_replace(true);
            }
            result
        });
        tokio::select! {biased;
         _=async {if !*stop.borrow(){let _=stop.changed().await;}}=>false,
         result=&mut reader=>result.is_ok_and(|r|r.is_ok()),
         result=&mut writer=>result.is_ok_and(|r|r.is_ok()),
        };
        if !reader.is_finished() {
            reader.abort();
            let _ = reader.await;
        }
        if !writer.is_finished() {
            writer.abort();
            let _ = writer.await;
        }
        // Both channel halves are gone; a full receive queue cannot strand the SDK event loop.
        transport
            .close_and_join()
            .await
            .map_err(|_| ProtocolSessionError::NativeUnknown)?;
        completion
            .wait()
            .await
            .map_err(|_| ProtocolSessionError::NativeUnknown)?;
        actor
            .join()
            .await
            .map_err(|_| ProtocolSessionError::NativeUnknown)?;
        // Logical transport errors do not replace proof of physical native completion.
        Ok(())
    })
}
