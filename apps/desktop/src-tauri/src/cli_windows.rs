//! Bounded public window labels only. These are routing metadata, never permission.
use serde_json::Value;
pub(super) fn decode(value: &Value) -> Option<Vec<String>> {
    let object = value.as_object()?;
    if object.len() != 2 || object.get("version")?.as_u64()? != 1 {
        return None;
    }
    let rows = object.get("windows")?.as_array()?;
    if rows.len() > 32 {
        return None;
    }
    let mut labels = Vec::new();
    for row in rows {
        let label = row.as_str()?;
        pulse_app::window_directory::WindowLabel::parse(label).ok()?;
        if labels.iter().any(|existing| existing == label) {
            return None;
        }
        labels.push(label.to_owned());
    }
    Some(labels)
}
