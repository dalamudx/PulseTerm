//! Pre-auth observation, intentionally separate from a verified/live transport.
use super::*;
use std::time::Duration;

/// Display-only result of the original backend trust policy, not approval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfferedKeyStatus {
    /// Neither validated source contains a pin for the selected target.
    Unknown,
    /// A known target conflicts; must not enter a first-use approval path.
    Changed,
    /// Existing sources match; observation still rejects the connection.
    AlreadyPinned,
}

/// Offered public key from one real handshake whose owned stream has exited.
/// Not a VerifiedPeer, trust-write token, credential permission or reconnect grant.
/// There is deliberately no public constructor, Clone, Debug or wire serialization.
/// ```compile_fail
/// fn dump(v:pulse_ssh::peer::ObservedHostKey){println!("{v:?}");}
/// ```
pub struct ObservedHostKey {
    target: HostTarget,
    key: PublicKey,
    status: OfferedKeyStatus,
}
impl ObservedHostKey {
    /// Original normalized target selected by the backend, not supplied by the peer.
    pub fn target(&self) -> &HostTarget {
        &self.target
    }
    /// Offered public key only; possession is neither trust nor credential authority.
    pub fn public_key(&self) -> &PublicKey {
        &self.key
    }
    /// Classification against the captured sources, not current source freshness.
    pub fn status(&self) -> OfferedKeyStatus {
        self.status
    }
}

struct Observer {
    target: HostTarget,
    own: Vec<HostPin>,
    system: Option<Vec<HostPin>>,
    offered: Arc<Mutex<Option<Result<ObservedHostKey, PeerError>>>>,
    shared: Arc<Shared>,
}
impl client::Handler for Observer {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        presented: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let value = match presented {
            PublicKeyOrCertificate::PublicKey { key, .. } => {
                let status = match verify_host_key(
                    &self.target,
                    key,
                    Some(&self.own),
                    self.system.as_deref(),
                ) {
                    Ok(()) => Ok(OfferedKeyStatus::AlreadyPinned),
                    Err(HostKeyError::Unknown) => Ok(OfferedKeyStatus::Unknown),
                    Err(HostKeyError::Changed) => Ok(OfferedKeyStatus::Changed),
                    Err(error) => Err(PeerError::HostKey(error)),
                };
                status.map(|status| ObservedHostKey {
                    target: self.target.clone(),
                    key: key.clone(),
                    status,
                })
            }
            _ => Err(PeerError::Certificate),
        };
        {
            let mut slot = self.offered.lock().map_err(|poison| {
                drop(poison);
                self.shared.close();
                russh::Error::UnknownKey
            })?;
            *slot = Some(if slot.is_some() {
                Err(PeerError::Stale)
            } else {
                value
            });
        }
        // Never accept a key, including an already pinned one; no auth handle exists.
        // Let SDK finish its normal rejection/shutdown: early IO revocation would
        // overwrite UnknownKey with BrokenPipe. Outer guard still closes on abort.
        Ok(false)
    }
}

/// Observe a backend-selected target without authenticating or trusting any offered key.
/// Sources must be independently loaded/validated. Changed keys are not first-use approval.
/// Returns only after owned I/O destruction, under the original total five-second deadline.
/// Cancellation closes pending I/O; the key-free reservation remains in the actual stream
/// until its destructor exits, even if the caller/runtime stops waiting.
/// `admission` must not contain a Vault key, plaintext or an available-key controller.
/// No DNS, filesystem discovery, trust writes, credentials or implicit reconnect occur here.
pub async fn observe_host_key<R>(
    target: HostTarget,
    own: Option<Vec<HostPin>>,
    system: Option<Vec<HostPin>>,
    config: Arc<client::Config>,
    admission: Arc<dyn Send + Sync>,
    stream: R,
) -> Result<ObservedHostKey, PeerError>
where
    R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let shared = Arc::new(Shared {
        io: native_io::IoLifetime::new(),
        wakeups: Mutex::new([None, None]),
        closed: AtomicBool::new(false),
        state: Mutex::new(State {
            claim: None,
            refusal: None,
        }),
    });
    let guard = CloseGuard(shared.clone());
    let stream = revocable_io::RevocableIo::new(stream, shared.clone());
    let completion = shared.io.retain(admission)?;
    let own = own.ok_or(PeerError::HostKey(HostKeyError::StoreUnavailable))?;
    if own.len() > crate::MAX_PINS
        || system
            .as_ref()
            .is_some_and(|pins| pins.len() > crate::MAX_PINS)
        || (system.is_none() && !own.iter().any(|pin| pin.target() == &target))
    {
        return Err(PeerError::HostKey(HostKeyError::StoreUnavailable));
    }
    let offered = Arc::new(Mutex::new(None));
    let handler = Observer {
        target,
        own,
        system,
        offered: offered.clone(),
        shared: shared.clone(),
    };
    let attempt =
        tokio::time::timeout_at(deadline, client::connect_stream(config, stream, handler)).await;
    shared.close();
    let refused = match attempt {
        Ok(Err(russh::Error::UnknownKey)) => true,
        Ok(Ok(handle)) => {
            drop(handle);
            false
        }
        _ => false,
    };
    tokio::time::timeout_at(deadline, completion.wait())
        .await
        .map_err(|_| PeerError::Transport)??;
    drop(guard);
    if !refused || tokio::time::Instant::now() >= deadline {
        return Err(PeerError::Transport);
    }
    let value = offered.lock().map_err(|_| PeerError::Stale)?.take();
    value.ok_or(PeerError::Transport)?
}
