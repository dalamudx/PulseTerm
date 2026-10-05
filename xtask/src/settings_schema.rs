//! Deterministic repository artifact, never a user settings writer.
use anyhow::{Result, ensure};
use pulse_store::settings::SettingsResolver;
const PATH: &str = "assets/schemas/settings.schema.json";
fn body() -> Result<String> {
    Ok(serde_json::to_string_pretty(&SettingsResolver::schema())? + "\n")
}
fn verify(actual: &str, expected: &str) -> Result<()> {
    ensure!(
        actual == expected,
        "Settings schema drift; run mise run settings-schema"
    );
    Ok(())
}
pub fn run(check: bool) -> Result<()> {
    let expected = body()?;
    if check {
        verify(&std::fs::read_to_string(PATH)?, &expected)?;
    } else {
        std::fs::create_dir_all("assets/schemas")?;
        std::fs::write(PATH, expected)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_mutated_schema_never_passes_a_check() -> Result<()> {
        let expected = body()?;
        verify(&expected, &expected)?;
        assert!(verify("", &expected).is_err());
        assert!(verify(&(expected.clone() + " "), &expected).is_err());
        assert!(expected.contains("\"maximum\": 72"));
        let mutated = expected.replace("\"maximum\": 72", "\"maximum\": 73");
        assert!(verify(&mutated, &expected).is_err());
        Ok(())
    }
}
