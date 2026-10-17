use super::*;
use serde::Deserialize;
use serde_json::json;
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FocusArgs {
    session: String,
}
impl ActionArgs for FocusArgs {
    fn validate(&self) -> Result<(), Error> {
        if self.session.is_empty()
            || self.session.len() > 20
            || !self.session.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(Error::Args);
        }
        Ok(())
    }
}
#[derive(Deserialize, JsonSchema)]
struct OpenArgs {
    _profile: String,
}
impl ActionArgs for OpenArgs {
    fn validate(&self) -> Result<(), Error> {
        Ok(())
    }
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ClosedOuterWithOpenNested {
    target: OpenArgs,
}
impl ActionArgs for ClosedOuterWithOpenNested {
    fn validate(&self) -> Result<(), Error> {
        self.target.validate()
    }
}
#[test]
fn nested_open_object_cannot_hide_unknown_parameter_loss_behind_a_closed_root() {
    let mut builder = Builder::default();
    assert_eq!(
        builder
            .register::<ClosedOuterWithOpenNested>(definition("workspace.open", &[Surface::Cli])),
        Err(Error::Definition)
    );
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ClosedNested {
    target: FocusArgs,
}
impl ActionArgs for ClosedNested {
    fn validate(&self) -> Result<(), Error> {
        self.target.validate()
    }
}
#[test]
fn nested_closed_schema_accepts_valid_values_but_rejects_nested_unknown_fields() -> Result<(), Error>
{
    let mut builder = Builder::default();
    builder.register::<ClosedNested>(definition("workspace.target", &[Surface::Cli]))?;
    let catalogue = builder.finish()?;
    catalogue.validate_args(
        "workspace.target",
        Surface::Cli,
        json!({"target":{"session":"1"}}),
    )?;
    assert_eq!(
        catalogue.validate_args(
            "workspace.target",
            Surface::Cli,
            json!({"target":{"session":"1","command":"exec"}})
        ),
        Err(Error::Args)
    );
    Ok(())
}
#[test]
fn action_count_budget_is_exact_and_failed_overflow_does_not_replace_the_catalogue()
-> Result<(), Error> {
    let mut builder = Builder::default();
    for index in 0..MAX_ACTIONS {
        let id: &'static str = Box::leak(format!("workspace.action.{index}").into_boxed_str());
        builder.register::<FocusArgs>(definition(id, &[Surface::Cli]))?;
    }
    assert_eq!(
        builder.register::<FocusArgs>(definition("workspace.overflow", &[Surface::Cli])),
        Err(Error::Budget)
    );
    let catalogue = builder.finish()?;
    assert_eq!(
        catalogue.projection(Surface::Cli)?["actions"]
            .as_array()
            .map(Vec::len),
        Some(MAX_ACTIONS)
    );
    Ok(())
}

#[test]
fn whole_catalogue_publication_budget_is_checked_even_when_each_schema_is_small()
-> Result<(), Error> {
    let mut builder = Builder::default();
    for index in 0..MAX_ACTIONS {
        let id: &'static str = Box::leak(format!("workspace.large.{index}").into_boxed_str());
        builder.register::<FocusArgs>(definition(id, &[Surface::Cli]))?;
        // Test-only mutation simulates a valid generated schema with a bounded long description.
        let entry = builder.entries.get_mut(id).ok_or(Error::Definition)?;
        entry.metadata.args["description"] = Value::String("x".repeat(1200));
        assert!(
            serde_json::to_vec(&entry.metadata.args)
                .map_err(|_| Error::Definition)?
                .len()
                < MAX_SCHEMA_BYTES
        );
    }
    assert!(matches!(builder.finish(), Err(Error::Budget)));
    Ok(())
}
#[test]
fn argument_errors_do_not_contain_untrusted_values() -> Result<(), Error> {
    let mut builder = Builder::default();
    builder.register::<FocusArgs>(definition("workspace.focus", &[Surface::Cli]))?;
    let catalogue = builder.finish()?;
    let result = catalogue.validate_args(
        "workspace.focus",
        Surface::Cli,
        json!({"session":"do-not-log-me"}),
    );
    assert_eq!(result, Err(Error::Args));
    assert!(!format!("{result:?}").contains("do-not-log-me"));
    assert_eq!(Error::Args.to_string(), "cap.invalid_args");
    Ok(())
}

fn definition(id: &'static str, surfaces: &'static [Surface]) -> ActionDef {
    ActionDef {
        id,
        title: "actions.focus.title",
        scope: Scope::Window,
        risk: Risk::Safe,
        surfaces,
    }
}
#[test]
fn generated_schema_and_surface_filtered_metadata_do_not_expose_handlers() -> Result<(), Error> {
    let mut builder = Builder::default();
    builder.register::<FocusArgs>(definition(
        "workspace.focus",
        &[Surface::Palette, Surface::Cli],
    ))?;
    let catalogue = builder.finish()?;
    let cli = catalogue.projection(Surface::Cli)?;
    assert_eq!(cli["version"], 1);
    assert_eq!(cli["actions"][0]["args"]["additionalProperties"], false);
    assert_eq!(cli["actions"][0]["scope"], "window");
    assert!(cli["actions"][0].get("handler").is_none());
    assert_eq!(catalogue.projection(Surface::Plugin)?["actions"], json!([]));
    Ok(())
}
#[test]
fn valid_arguments_are_not_a_target_or_execution_permission() -> Result<(), Error> {
    let mut builder = Builder::default();
    builder.register::<FocusArgs>(definition("workspace.focus", &[Surface::Cli]))?;
    let catalogue = builder.finish()?;
    catalogue.validate_args("workspace.focus", Surface::Cli, json!({"session":"123"}))?;
    assert_eq!(
        catalogue.validate_args("workspace.focus", Surface::Plugin, json!({"session":"123"})),
        Err(Error::Unavailable)
    );
    assert_eq!(
        catalogue.validate_args("shell.exec", Surface::Cli, json!({})),
        Err(Error::Unavailable)
    );
    for args in [
        json!({"session":"123","path":"/bin/sh"}),
        json!({"session":""}),
        json!({"session":123}),
        json!({}),
        json!([]),
        json!({"session":"a"}),
    ] {
        assert_eq!(
            catalogue.validate_args("workspace.focus", Surface::Cli, args),
            Err(Error::Args)
        );
    }
    Ok(())
}
#[test]
fn duplicate_or_invalid_registration_never_replaces_existing_action() -> Result<(), Error> {
    let mut builder = Builder::default();
    builder.register::<FocusArgs>(definition("workspace.focus", &[Surface::Cli]))?;
    assert_eq!(
        builder.register::<FocusArgs>(definition("workspace.focus", &[Surface::Plugin])),
        Err(Error::Definition)
    );
    for id in ["", "command with space", "../exec", "命令"] {
        assert_eq!(
            builder.register::<FocusArgs>(definition(id, &[Surface::Cli])),
            Err(Error::Definition)
        );
    }
    assert_eq!(
        builder.register::<FocusArgs>(definition("empty", &[])),
        Err(Error::Definition)
    );
    assert_eq!(
        builder.register::<FocusArgs>(definition(
            "duplicate.surface",
            &[Surface::Cli, Surface::Cli]
        )),
        Err(Error::Definition)
    );
    let catalogue = builder.finish()?;
    assert_eq!(
        catalogue.projection(Surface::Cli)?["actions"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(catalogue.projection(Surface::Plugin)?["actions"], json!([]));
    Ok(())
}
#[test]
fn open_object_argument_schema_is_rejected_instead_of_silently_dropping_unknown_fields() {
    let mut builder = Builder::default();
    assert_eq!(
        builder.register::<OpenArgs>(definition("workspace.open", &[Surface::Cli])),
        Err(Error::Definition)
    );
}
#[test]
fn argument_control_budget_is_checked_before_typed_validation() -> Result<(), Error> {
    let mut builder = Builder::default();
    builder.register::<FocusArgs>(definition("workspace.focus", &[Surface::Cli]))?;
    let catalogue = builder.finish()?;
    assert_eq!(
        catalogue.validate_args(
            "workspace.focus",
            Surface::Cli,
            json!({"session":"1".repeat(MAX_ARGS_BYTES)})
        ),
        Err(Error::Budget)
    );
    Ok(())
}
#[test]
fn catalogue_risk_is_metadata_not_an_approved_gui_confirmation() -> Result<(), Error> {
    for risk in [Risk::Safe, Risk::Confirm, Risk::Dangerous] {
        let mut builder = Builder::default();
        let mut action = definition("workspace.focus", &[Surface::Cli]);
        action.risk = risk;
        builder.register::<FocusArgs>(action)?;
        let catalogue = builder.finish()?;
        assert_eq!(
            catalogue.projection(Surface::Cli)?["actions"][0]["risk"],
            serde_json::to_value(risk).map_err(|_| Error::Definition)?
        );
        catalogue.validate_args("workspace.focus", Surface::Cli, json!({"session":"1"}))?;
    }
    Ok(())
}
