use super::*;
use crate::{product_document::ProductDocuments, registry::WindowToken};
use pulse_cap::{ActionArgs, ActionDef, Builder};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Args {
    value: u16,
}
impl ActionArgs for Args {
    fn validate(&self) -> Result<(), pulse_cap::Error> {
        if self.value > 100 {
            return Err(pulse_cap::Error::Args);
        }
        Ok(())
    }
}
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn catalogue(risk: Risk, scope: Scope) -> Result<Catalogue, pulse_cap::Error> {
    let mut builder = Builder::default();
    builder.register::<Args>(ActionDef {
        id: "workspace.test",
        title: "workspace.test.title",
        scope,
        risk,
        surfaces: &[Surface::Cli],
    })?;
    builder.finish()
}
type Fixture = (
    SessionRegistry,
    ProductDocuments,
    WindowToken,
    Arc<PageLaunchScope>,
);
fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window()?;
    documents.install(owner)?;
    documents.started(owner)?;
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let page = documents.current(owner, nonce)?;
    Ok((registry, documents, owner, page))
}
#[test]
fn removing_actual_document_retires_pending_and_confirmed_requests() -> TestResult {
    let (registry, docs, owner, page) = fixture()?;
    let catalogue = catalogue(Risk::Confirm, Scope::Window)?;
    let pending = prepared(&catalogue, &registry, &page)?;
    let confirmed = prepared(&catalogue, &registry, &page)?.confirm(&page)?;
    docs.remove(owner)?;
    assert!(matches!(pending.confirm(&page), Err(Error::Forbidden)));
    assert!(matches!(
        confirmed.take_args(&page, "workspace.test"),
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        prepared(&catalogue, &registry, &page),
        Err(Error::Forbidden)
    ));
    Ok(())
}
#[test]
fn confirmation_does_not_renew_original_deadline_or_replace_immutable_parameters() -> TestResult {
    let (registry, _docs, _owner, page) = fixture()?;
    let catalogue = catalogue(Risk::Confirm, Scope::Window)?;
    let request = prepared(&catalogue, &registry, &page)?;
    let original_deadline = request.deadline;
    let request = request.confirm(&page)?;
    assert_eq!(request.deadline, original_deadline);
    assert_eq!(
        request.take_args(&page, "workspace.test")?,
        json!({"value":7})
    );
    Ok(())
}

fn prepared(
    catalogue: &Catalogue,
    registry: &SessionRegistry,
    page: &Arc<PageLaunchScope>,
) -> Result<PreparedWindowAction, Error> {
    PreparedWindowAction::prepare(
        catalogue,
        registry,
        page.clone(),
        "workspace.test",
        Surface::Cli,
        json!({"value":7}),
    )
}
#[test]
fn safe_original_request_is_consumed_as_parameters_not_an_effect() -> TestResult {
    let (registry, _docs, _owner, page) = fixture()?;
    let catalogue = catalogue(Risk::Safe, Scope::Window)?;
    assert_eq!(
        prepared(&catalogue, &registry, &page)?.take_args(&page, "workspace.test")?,
        json!({"value":7})
    );
    assert!(matches!(
        prepared(&catalogue, &registry, &page)?.confirm(&page),
        Err(Error::Confirmation)
    ));
    Ok(())
}
#[test]
fn confirm_and_dangerous_require_exact_original_gui_confirmation() -> TestResult {
    let (registry, _docs, _owner, page) = fixture()?;
    for risk in [Risk::Confirm, Risk::Dangerous] {
        let catalogue = catalogue(risk, Scope::Window)?;
        assert!(matches!(
            prepared(&catalogue, &registry, &page)?.take_args(&page, "workspace.test"),
            Err(Error::Confirmation)
        ));
        let request = prepared(&catalogue, &registry, &page)?.confirm(&page)?;
        assert_eq!(
            request.take_args(&page, "workspace.test")?,
            json!({"value":7})
        );
        assert!(matches!(
            prepared(&catalogue, &registry, &page)?
                .confirm(&page)?
                .confirm(&page),
            Err(Error::Confirmation)
        ));
    }
    Ok(())
}
#[test]
fn actual_document_navigation_cannot_upgrade_old_confirmation_or_take_args() -> TestResult {
    let (registry, docs, owner, page) = fixture()?;
    let catalogue = catalogue(Risk::Confirm, Scope::Window)?;
    let pending = prepared(&catalogue, &registry, &page)?;
    let confirmed = prepared(&catalogue, &registry, &page)?.confirm(&page)?;
    docs.started(owner)?;
    let nonce = docs.finished(owner)?;
    docs.confirm(owner, nonce)?;
    let current = docs.current(owner, nonce)?;
    assert!(matches!(pending.confirm(&current), Err(Error::Forbidden)));
    assert!(matches!(
        confirmed.take_args(&page, "workspace.test"),
        Err(Error::Forbidden)
    ));
    assert_eq!(
        prepared(&catalogue, &registry, &current)?
            .confirm(&current)?
            .take_args(&current, "workspace.test")?,
        json!({"value":7})
    );
    Ok(())
}
#[test]
fn foreign_registry_and_other_window_cannot_replace_original_target() -> TestResult {
    let (registry, _docs, _owner, page) = fixture()?;
    let (foreign, _foreign_docs, _foreign_owner, other) = fixture()?;
    let catalogue = catalogue(Risk::Confirm, Scope::Window)?;
    assert!(matches!(
        prepared(&catalogue, &foreign, &page),
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        prepared(&catalogue, &registry, &page)?.confirm(&other),
        Err(Error::Forbidden)
    ));
    let owner = registry.open_window()?;
    let docs = ProductDocuments::new(registry.clone());
    docs.install(owner)?;
    docs.started(owner)?;
    let nonce = docs.finished(owner)?;
    docs.confirm(owner, nonce)?;
    let same_registry_other_page = docs.current(owner, nonce)?;
    assert!(matches!(
        prepared(&catalogue, &registry, &page)?.confirm(&same_registry_other_page),
        Err(Error::Forbidden)
    ));
    Ok(())
}
#[test]
fn expected_action_mismatch_and_original_deadline_cannot_be_replaced() -> TestResult {
    let (registry, _docs, _owner, page) = fixture()?;
    let catalogue = catalogue(Risk::Confirm, Scope::Window)?;
    assert!(matches!(
        prepared(&catalogue, &registry, &page)?
            .confirm(&page)?
            .take_args(&page, "workspace.other"),
        Err(Error::Forbidden)
    ));
    let mut expired = prepared(&catalogue, &registry, &page)?;
    expired.deadline = Instant::now();
    assert!(matches!(expired.confirm(&page), Err(Error::Expired)));
    let mut expired = prepared(&catalogue, &registry, &page)?.confirm(&page)?;
    expired.deadline = Instant::now();
    assert!(matches!(
        expired.take_args(&page, "workspace.test"),
        Err(Error::Expired)
    ));
    Ok(())
}
#[test]
fn unsupported_scopes_surface_unknowns_and_semantic_args_cannot_prepare() -> TestResult {
    let (registry, _docs, _owner, page) = fixture()?;
    for scope in [Scope::Global, Scope::Session] {
        assert!(matches!(
            prepared(&catalogue(Risk::Safe, scope)?, &registry, &page),
            Err(Error::Forbidden)
        ));
    }
    let catalogue = catalogue(Risk::Safe, Scope::Window)?;
    for args in [
        json!({"value":101}),
        json!({"value":1,"path":"/bin/sh"}),
        json!({}),
        json!([]),
    ] {
        assert!(matches!(
            PreparedWindowAction::prepare(
                &catalogue,
                &registry,
                page.clone(),
                "workspace.test",
                Surface::Cli,
                args
            ),
            Err(Error::Request)
        ));
    }
    assert!(matches!(
        PreparedWindowAction::prepare(
            &catalogue,
            &registry,
            page.clone(),
            "workspace.test",
            Surface::Plugin,
            json!({"value":1})
        ),
        Err(Error::Request)
    ));
    Ok(())
}
