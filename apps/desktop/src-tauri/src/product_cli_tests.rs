//! Exact native original-scope checks used before read and before publication.
use super::*;
use pulse_app::{product_document::ProductDocuments, registry::SessionRegistry};

#[test]
fn cli_queued_or_post_read_result_cannot_publish_for_replacement_page()
-> Result<(), Box<dyn std::error::Error>> {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window()?;
    documents.install(owner)?;
    documents.started(owner)?;
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let original = documents.current_native(owner)?;
    let token = CancellationToken::new();
    assert!(original_current(&documents, owner, &original, &token).is_ok());
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(original_current(&documents, owner, &original, &cancelled).is_err());

    let other = registry.open_window()?;
    documents.install(other)?;
    documents.started(other)?;
    let other_nonce = documents.finished(other)?;
    documents.confirm(other, other_nonce)?;
    assert!(original_current(&documents, other, &original, &token).is_err());
    let foreign_registry = SessionRegistry::new();
    let foreign = ProductDocuments::new(foreign_registry.clone());
    let foreign_owner = foreign_registry.open_window()?;
    foreign.install(foreign_owner)?;
    foreign.started(foreign_owner)?;
    let foreign_nonce = foreign.finished(foreign_owner)?;
    foreign.confirm(foreign_owner, foreign_nonce)?;
    assert!(original_current(&foreign, foreign_owner, &original, &token).is_err());

    documents.started(owner)?;
    assert!(original_current(&documents, owner, &original, &token).is_err());
    let nonce = documents.finished(owner)?;
    assert!(documents.current_native(owner).is_err());
    documents.confirm(owner, nonce)?;
    let fresh = documents.current_native(owner)?;
    assert!(original_current(&documents, owner, &original, &token).is_err());
    assert!(original_current(&documents, owner, &fresh, &token).is_ok());
    documents.remove(owner)?;
    assert!(original_current(&documents, owner, &fresh, &token).is_err());
    Ok(())
}
