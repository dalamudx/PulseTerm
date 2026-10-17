use crate::{
    StoreError,
    vault_crypto::VaultCipher,
    vault_database::{self, VaultEdit},
    vault_record::CredentialRecord,
    vault_wrap::ValidationWrappedDek,
};
use rusqlite::Connection;
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn database() -> Result<(Connection, Uuid, Uuid, Uuid), Box<dyn std::error::Error>> {
    let c = Connection::open_in_memory()?;
    c.execute_batch(
        "CREATE TABLE app_state(key TEXT PRIMARY KEY NOT NULL,value TEXT NOT NULL) STRICT;",
    )?;
    let owner = Uuid::new_v4();
    let vault = Uuid::now_v7();
    let credential = Uuid::now_v7();
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(vault.as_bytes());
    let envelope = ValidationWrappedDek::decode_ciphertext(&bytes, vault)?;
    let empty = vault_database::load(&c, owner)?;
    let saved = vault_database::edit(
        &c,
        owner,
        &empty,
        VaultEdit::Initialize(vault, envelope),
        || true,
    )?;
    let cipher = VaultCipher::generate()?;
    let record =
        CredentialRecord::seal(vault, credential, "ssh_password", &cipher, b"fixture-data")?;
    vault_database::edit(&c, owner, &saved, VaultEdit::Create(record), || true)?;
    Ok((c, owner, vault, credential))
}
fn read(c: &Connection) -> rusqlite::Result<String> {
    c.query_row(
        "SELECT value FROM app_state WHERE key=?1",
        [vault_database::KEY],
        |r| r.get(0),
    )
}
fn write(c: &Connection, value: &str) -> rusqlite::Result<usize> {
    c.execute(
        "UPDATE app_state SET value=?1 WHERE key=?2",
        rusqlite::params![value, vault_database::KEY],
    )
}
#[test]
fn corrupt_non_target_duplicate_future_unknown_and_budgets_reject_entire_source() -> TestResult {
    let (c, owner, _, _) = database()?;
    let original = read(&c)?;
    let valid: serde_json::Value = serde_json::from_str(&original)?;
    let mut cases = Vec::new();
    let mut v = valid.clone();
    v["version"] = 2.into();
    cases.push(v);
    let mut v = valid.clone();
    v["extra"] = true.into();
    cases.push(v);
    let mut v = valid.clone();
    v["revision"] = 0.into();
    cases.push(v);
    let mut v = valid.clone();
    v["credentials"][0][39] = (b'S' as u64).into();
    cases.push(v);
    let mut v = valid.clone();
    v["credentials"] = serde_json::Value::Array(vec![valid["credentials"][0].clone(); 257]);
    cases.push(v);
    let mut v = valid.clone();
    v["credentials"] = serde_json::Value::Array(vec![valid["credentials"][0].clone(); 2]);
    cases.push(v);
    let mut v = valid.clone();
    v["deleted"] =
        serde_json::Value::Array(vec![serde_json::Value::String(Uuid::nil().to_string())]);
    cases.push(v);
    for value in cases {
        let text = serde_json::to_string(&value)?;
        write(&c, &text)?;
        assert!(matches!(
            vault_database::load(&c, owner),
            Err(StoreError::VaultRecord)
        ));
        assert_eq!(read(&c)?, text);
    }
    let excessive = " ".repeat(4_194_305);
    write(&c, &excessive)?;
    assert!(matches!(
        vault_database::load(&c, owner),
        Err(StoreError::VaultRecord)
    ));
    assert_eq!(read(&c)?.len(), 4_194_305);
    Ok(())
}
#[test]
fn exhausted_revision_and_deleted_limit_never_wrap_commit_or_drop_tombstones() -> TestResult {
    let (c, owner, _, credential) = database()?;
    let mut value: serde_json::Value = serde_json::from_str(&read(&c)?)?;
    value["revision"] = (i64::MAX as u64).into();
    let text = serde_json::to_string(&value)?;
    write(&c, &text)?;
    let snapshot = vault_database::load(&c, owner)?;
    assert!(matches!(
        vault_database::edit(&c, owner, &snapshot, VaultEdit::Remove(credential), || true),
        Err(StoreError::VaultRevision)
    ));
    assert_eq!(read(&c)?, text);
    value["revision"] = 2.into();
    value["deleted"] = serde_json::Value::Array(
        (0..4096)
            .map(|_| serde_json::Value::String(Uuid::now_v7().to_string()))
            .collect(),
    );
    let text = serde_json::to_string(&value)?;
    write(&c, &text)?;
    let snapshot = vault_database::load(&c, owner)?;
    assert!(matches!(
        vault_database::edit(&c, owner, &snapshot, VaultEdit::Remove(credential), || true),
        Err(StoreError::VaultRecord)
    ));
    assert_eq!(read(&c)?, text);
    Ok(())
}
