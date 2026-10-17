use super::*;
use std::os::unix::fs::PermissionsExt;
#[test]
fn fixed_sources_reopen_complete_distinct_documents_and_close_independently()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    let (a, b, documents) = files(dir.path().to_path_buf())?;
    assert!(
        documents[0] != documents[1]
            && documents[0] != documents[2]
            && documents[1] != documents[2]
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let _serial = THEME_IO_TEST.lock().await;
        let first = a.handle().begin_load()?.ready().await?;
        let second = b.handle().begin_load()?.ready().await?;
        assert!(first.document()?.as_ref() == Some(&documents[0]));
        assert!(second.document()?.as_ref() == Some(&documents[1]));
        a.close();
        assert!(!a.handle().is_open() && b.handle().is_open());
        assert!(matches!(
            a.handle().begin_load(),
            Err(pulse_store::theme_io::ThemeIoError::Closed)
        ));
        let still = b.handle().begin_load()?.ready().await?;
        assert!(still.document()?.as_ref() == Some(&documents[1]));
        b.close();
        Ok(())
    })
}
