//! Fixed original-page action/source/key approval validation, never a product network entry.
use super::*;
use pulse_app::registry::trust_action::{PageTrustActions, TrustActionError};
use pulse_ssh::managed_store::{FirstPinApproval, FirstPinError};
pub fn run() -> Result<()> {
    tokio::runtime::Builder::new_current_thread().enable_all().max_blocking_threads(4).build()?.block_on(async {
        let registry=SessionRegistry::new();let documents=Arc::new(ProductDocuments::new(registry.clone()));
        let window=registry.open_window()?;documents.install(window)?;documents.started(window)?;
        let nonce=documents.finished(window)?;documents.confirm(window,nonce)?;
        let page=documents.current(window,nonce)?;
        let actions=Arc::new(PageTrustActions::new(registry.clone(),window));
        let dir=tempfile::tempdir()?;use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(),std::fs::Permissions::from_mode(0o700))?;
        let path=dir.path().join("state.db");let(store,worker)=StateStore::open(path.clone()).await?;
        let secret=russh::keys::PrivateKey::new(russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[73;32]).into(),"")?;
        let key=secret.public_key().clone();let config=Arc::new(server::Config{keys:vec![secret],..Default::default()});
        let listener=tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).await?;let address=listener.local_addr()?;
        let auth=Arc::new(AtomicUsize::new(0));let mut spy=Spy{auth:auth.clone()};let(tx,rx)=tokio::sync::oneshot::channel();
        let task=tokio::spawn(async move{let running=spy.run_on_socket(config,&listener);let _sent=tx.send(running.handle());running.await});
        let mut server=ServerGuard{stop:rx.await?,task:Some(task)};
        let hosts=store.load_ssh_hosts().await?;
        let hosts=store.edit_ssh_hosts(hosts,HostEdit::Create(SshEndpoint::new("fixed","127.0.0.1",address.port(),"user")?),Arc::new(||true)).await?;
        let host=hosts.records()[0].id();let config=Arc::new(russh::client::Config::default());
        let foreign=SessionRegistry::new();let _window=foreign.open_window()?;
        ensure!(foreign.check_launch_page(&page).is_err(),"first_pin.foreign_registry");foreign.shutdown()?;
        let action=actions.begin(page.clone(),host)?;let cancelled=action.cancellation();let original=action.authorization();
        let source=store.load_ssh_target_source(original.clone()).await?;
        let prompt=FirstPinApproval::observe(source,action.host(),Some(vec![]),config.clone(),original,tokio::net::TcpStream::connect(address).await?).await?;
        ensure!(prompt.candidate().public_key()==&key,"first_pin.offered_key");
        ensure!(matches!(actions.begin(page.clone(),host),Err(TrustActionError::Busy)),"first_pin.action_duplicate_busy");
        documents.started(window)?;actions.retire();cancelled.wait().await;
        ensure!(matches!(prompt.confirm(&store,true).await,Err(FirstPinError::Denied)),"first_pin.retired_page");
        ensure!(store.load_ssh_trust().await?.revision()==0,"first_pin.old_page_write");drop(action);
        let nonce=documents.finished(window)?;documents.confirm(window,nonce)?;let page=documents.current(window,nonce)?;
        let action=actions.begin(page,host)?;let native=action.authorization();
        let source=store.load_ssh_target_source(native.clone()).await?;
        let calls=Arc::new(AtomicUsize::new(0));let count=calls.clone();let doc=documents.clone();let retirement=actions.clone();
        let final_guard:Arc<dyn Fn()->bool+Send+Sync>=Arc::new(move||{
            if count.fetch_add(1,Ordering::SeqCst)==2 {let _retired=doc.started(window);retirement.retire();}
            native()
        });
        calls.store(usize::MAX-16,Ordering::SeqCst);
        let prompt=FirstPinApproval::observe(source,action.host(),Some(vec![]),config.clone(),final_guard,tokio::net::TcpStream::connect(address).await?).await?;
        calls.store(0,Ordering::SeqCst);
        ensure!(matches!(prompt.confirm(&store,true).await,Err(FirstPinError::Source)),"first_pin.final_page_gate");
        ensure!(calls.load(Ordering::SeqCst)==3&&store.load_ssh_trust().await?.revision()==0,"first_pin.final_rollback");drop(action);
        let nonce=documents.finished(window)?;documents.confirm(window,nonce)?;let page=documents.current(window,nonce)?;
        let action=actions.begin(page,host)?;let current=action.authorization();
        let source=store.load_ssh_target_source(current.clone()).await?;
        let prompt=FirstPinApproval::observe(source,action.host(),Some(vec![]),config.clone(),current,tokio::net::TcpStream::connect(address).await?).await?;
        ensure!(prompt.confirm(&store,true).await?==1,"first_pin.current_confirm");drop(action);
        worker.shutdown().await?;let(reopened,worker)=StateStore::open(path).await?;
        let source=reopened.load_ssh_target_source(Arc::new(||true)).await?;let pins=validated_pins(source.trust())?;
        let target=HostTarget::new("127.0.0.1",address.port())?;
        let transport=PinnedTransport::connect_stream(target,Some(pins),None,config,tokio::net::TcpStream::connect(address).await?).await?;
        ensure!(peer_guard(&source,host,&transport)?.as_ref()(),"first_pin.reopened_strict_peer");
        let done=transport.native_io_completion();drop(transport);tokio::time::timeout(std::time::Duration::from_secs(2),done.wait()).await??;
        ensure!(auth.load(Ordering::SeqCst)==0,"first_pin.auth_not_zero");worker.shutdown().await?;
        server.stop.shutdown("fixed first pin end".into());if let Some(task)=server.task.take(){tokio::time::timeout(std::time::Duration::from_secs(2),task).await???;}
        actions.close();registry.close_window(window)?;registry.shutdown()?;
        std::fs::write(crate::root()?.join("target/probes/ssh-first-pin.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion":1,"actualOfferedKey":true,"foreignRegistryRejected":true,"retiredPageRejected":true,
            "finalPageRevocationRolledBack":true,"currentPageApproval":true,"reopenedPinStrictPeer":true,
            "originalActionScoped":true,"cancelSignalChecked":true,"actionAdmissionReclaimed":true,
            "authenticationRequests":0,"productSshOpened":false
        }))?)?;
        println!("ssh.first_use_actual_key_original_page_atomic_pin_reopen_confirmed auth=0 product_session=false");Ok(())
    })
}
