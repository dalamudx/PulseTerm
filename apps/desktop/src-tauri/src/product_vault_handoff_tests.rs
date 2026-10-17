use super::*;
#[test]
fn canonical_ticket_and_intent_reject_raw_secret_or_cross_protocol_fields()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let id = Uuid::now_v7();
    assert_eq!(ticket(&id.to_string())?, id);
    for value in [
        String::new(),
        id.to_string().to_uppercase(),
        Uuid::new_v4().to_string(),
        Uuid::nil().to_string(),
        id.simple().to_string(),
        "a".repeat(1024 * 1024),
    ] {
        assert!(ticket(&value).is_err());
    }
    for value in [
        serde_json::json!(true),
        serde_json::json!("password"),
        serde_json::json!({"kind":"unlock","password":[1]}),
        serde_json::json!(["create"]),
    ] {
        assert!(serde_json::from_value::<Kind>(value).is_err());
    }
    assert!(matches!(
        serde_json::from_value::<Kind>(serde_json::json!("create"))?,
        Kind::Create
    ));
    assert!(matches!(
        serde_json::from_value::<Kind>(serde_json::json!("unlock"))?,
        Kind::Unlock
    ));
    Ok(())
}
