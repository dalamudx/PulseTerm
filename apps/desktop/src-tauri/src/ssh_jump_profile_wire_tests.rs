use super::*;
#[test]
fn jump_wire_distinguishes_preserve_clear_and_set_and_rejects_ambiguous_fields()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let profile = Uuid::now_v7();
    let via = Uuid::now_v7();
    let legacy =
        serde_json::json!({"id":profile,"label":"Profile","username":"u","identityId":null});
    assert!(
        serde_json::from_value::<Change>(legacy.clone())?
            .edit()?
            .1
            .is_none()
    );
    let mut routed = legacy.clone();
    routed["version"] = serde_json::json!(2);
    routed["via"] = serde_json::Value::Null;
    assert_eq!(
        serde_json::from_value::<Change>(routed.clone())?.edit()?.1,
        Some(None)
    );
    routed["via"] = serde_json::json!(via);
    assert_eq!(
        serde_json::from_value::<Change>(routed.clone())?.edit()?.1,
        Some(Some(via))
    );
    let mut missing = routed.clone();
    missing
        .as_object_mut()
        .ok_or("fixture.object")?
        .remove("via");
    assert!(serde_json::from_value::<Change>(missing).is_err());
    let mut injected = legacy;
    injected["via"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<Change>(injected).is_err());
    for value in [
        serde_json::json!({}),
        serde_json::json!(true),
        serde_json::json!(Uuid::new_v4()),
        serde_json::json!(via.to_string().to_uppercase()),
    ] {
        let mut malformed = routed.clone();
        malformed["via"] = value;
        assert!(serde_json::from_value::<Change>(malformed)?.edit().is_err());
    }
    routed["version"] = serde_json::json!(3);
    assert!(serde_json::from_value::<Change>(routed)?.edit().is_err());
    Ok(())
}
