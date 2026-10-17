#![allow(clippy::unwrap_used)]
use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn request(method: &str, params: Value) -> Request {
    Request::parse(
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"test","method":method,"params":params}))
            .unwrap()
            .as_slice(),
    )
    .unwrap()
}
fn window(
    application: &Arc<super::super::super::Application>,
    contexts: &super::super::super::window_state::Contexts<Workspace>,
    primary: bool,
) -> Workspace {
    let registration = Arc::new(application.directory.reserve(primary).unwrap());
    let native = Arc::new(
        super::super::super::app::NativeBinding::new(application.directory.clone(), registration)
            .unwrap(),
    );
    let reader = Arc::new(super::super::super::profiles::ProfileReader::with_storage(
        application.profile_storage.clone(),
    ));
    let state = Workspace::from_native(application.clone(), native.clone(), reader).unwrap();
    contexts
        .install(native.label().as_str(), state.clone())
        .unwrap();
    native.publish().unwrap();
    application.documents.started(state.0.owner).unwrap();
    let nonce = application.documents.finished(state.0.owner).unwrap();
    application.documents.confirm(state.0.owner, nonce).unwrap();
    state
}
async fn close(
    application: &Arc<super::super::super::Application>,
    states: &[Workspace],
) -> TestResult {
    for state in states {
        state.0.native.as_ref().unwrap().destroyed()?;
    }
    application.window_drains.shutdown().await?;
    application.profile_storage.shutdown().await?;
    application.settings_io.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn list_is_safe_closed_metadata_even_with_multiple_windows_and_no_page_approval() -> TestResult
{
    let application = super::super::super::app::tests::application();
    let contexts = super::super::super::window_state::Contexts::new();
    let a = window(&application, &contexts, true);
    let b = window(&application, &contexts, false);
    let value = list(
        &application.directory,
        &request("workspace.windows", json!({})),
    )?;
    assert_eq!(projection::decode(&value).unwrap().len(), 2);
    for params in [
        json!({"window":"workspace"}),
        json!({"token":"x"}),
        json!({"all":true}),
    ] {
        assert!(
            list(
                &application.directory,
                &request("workspace.windows", params)
            )
            .is_err()
        );
    }
    assert!(
        list(
            &application.directory,
            &request("workspace.profiles", json!({}))
        )
        .is_err()
    );
    application.documents.started(a.0.owner)?;
    assert_eq!(
        projection::decode(&list(
            &application.directory,
            &request("workspace.windows", json!({}))
        )?)
        .unwrap()
        .len(),
        2
    );
    close(&application, &[a, b]).await
}
#[tokio::test]
async fn exact_selector_routes_original_page_and_strips_metadata_without_consuming_approval()
-> TestResult {
    let application = super::super::super::app::tests::application();
    let contexts = super::super::super::window_state::Contexts::new();
    let a = window(&application, &contexts, true);
    assert!(
        resolve(
            &application.directory,
            &contexts,
            request("workspace.new-tab", json!({"profile":"sh"}))
        )
        .is_ok()
    );
    let b = window(&application, &contexts, false);
    assert!(
        resolve(
            &application.directory,
            &contexts,
            request("workspace.new-tab", json!({"profile":"sh"}))
        )
        .is_err()
    );
    let (chosen, action, page) = resolve(
        &application.directory,
        &contexts,
        request(
            "workspace.new-tab",
            json!({"profile":"sh","window":b.native_label()?}),
        ),
    )?;
    assert!(Arc::ptr_eq(&chosen.0, &b.0));
    assert_eq!(action.params().len(), 1);
    let prepared = PreparedWindowAction::prepare(
        &capabilities::catalogue(true)?,
        &application.registry,
        page.clone(),
        action.method(),
        Surface::Cli,
        Value::Object(action.params().clone()),
    )?;
    assert!(prepared.take_args(&page, "workspace.new-tab").is_err());
    application.documents.started(b.0.owner)?;
    assert!(
        original_current(
            &application.documents,
            b.0.owner,
            &page,
            &CancellationToken::new()
        )
        .is_err()
    );
    assert!(
        resolve(
            &application.directory,
            &contexts,
            request("workspace.focus", json!({"window":b.native_label()?}))
        )
        .is_err()
    );
    assert!(
        resolve(
            &application.directory,
            &contexts,
            request("workspace.focus", json!({"window":a.native_label()?}))
        )
        .is_ok()
    );
    close(&application, &[a, b]).await
}
#[tokio::test]
async fn unknown_invalid_retired_and_foreign_context_cannot_fall_back() -> TestResult {
    let application = super::super::super::app::tests::application();
    let contexts = super::super::super::window_state::Contexts::new();
    let a = window(&application, &contexts, true);
    let b = window(&application, &contexts, false);
    for label in [
        json!("workspace-x"),
        json!(format!("workspace-{}", uuid::Uuid::new_v4())),
        json!(null),
        json!(1),
    ] {
        assert!(
            resolve(
                &application.directory,
                &contexts,
                request("workspace.focus", json!({"window":label}))
            )
            .is_err()
        );
    }
    let foreign_application = super::super::super::app::tests::application();
    let foreign_contexts = super::super::super::window_state::Contexts::new();
    let foreign = window(&foreign_application, &foreign_contexts, true);
    assert!(
        resolve(
            &application.directory,
            &foreign_contexts,
            request("workspace.focus", json!({"window":"workspace"}))
        )
        .is_err()
    );
    a.0.native.as_ref().unwrap().destroyed()?;
    assert!(
        resolve(
            &application.directory,
            &contexts,
            request("workspace.focus", json!({"window":"workspace"}))
        )
        .is_err()
    );
    assert!(
        resolve(
            &application.directory,
            &contexts,
            request("workspace.focus", json!({}))
        )
        .is_ok()
    );
    close(&application, &[a, b]).await?;
    close(&foreign_application, &[foreign]).await
}
#[test]
fn client_projection_rejects_permissions_duplicates_and_unbounded_or_noncanonical_labels() {
    for value in [
        json!({"version":1,"windows":["workspace"],"token":"x"}),
        json!({"version":1,"windows":["workspace","workspace"]}),
        json!({"version":2,"windows":[]}),
        json!({"version":1,"windows":["workspace-x"]}),
        json!({"version":1,"windows":vec!["workspace";33]}),
    ] {
        assert!(projection::decode(&value).is_none());
    }
}
