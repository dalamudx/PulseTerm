use super::{Rgb8, TerminalPalette, TerminalScheme, ThemeError, budget, validate_name};
use quick_xml::{Reader, events::Event};
use serde::{
    Deserialize,
    de::{self, MapAccess, Visitor},
};
use std::{collections::BTreeMap, fmt};
const DTD: &str = "plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Components {
    #[serde(rename = "Red Component")]
    red: f64,
    #[serde(rename = "Green Component")]
    green: f64,
    #[serde(rename = "Blue Component")]
    blue: f64,
    #[serde(rename = "Color Space")]
    space: String,
    #[serde(rename = "Alpha Component", default)]
    alpha: Option<f64>,
}
impl Components {
    fn rgb(self) -> Result<Rgb8, ThemeError> {
        if self.space != "sRGB" || self.alpha.is_some_and(|a| a != 1.0) {
            return Err(ThemeError::Iterm);
        }
        let channels = [self.red, self.green, self.blue];
        if channels
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(ThemeError::Iterm);
        }
        let c = channels.map(|v| (v * 255.0).round() as u8);
        Ok(Rgb8::new(c[0], c[1], c[2]))
    }
}
struct Wire(BTreeMap<String, Components>);
impl<'de> Deserialize<'de> for Wire {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Wire;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("unique color dictionary")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut m: M) -> Result<Wire, M::Error> {
                let mut colors = BTreeMap::new();
                while let Some((k, v)) = m.next_entry::<String, Components>()? {
                    if colors.len() >= 22 || colors.insert(k, v).is_some() {
                        return Err(de::Error::custom("invalid palette"));
                    }
                }
                Ok(Wire(colors))
            }
        }
        d.deserialize_map(V)
    }
}
fn preflight(text: &str) -> Result<(), ThemeError> {
    let mut r = Reader::from_str(text);
    r.config_mut().check_end_names = true;
    let mut stack: Vec<String> = Vec::new();
    let (mut root, mut done, mut declaration, mut dtd) = (false, false, false, false);
    for _ in 0..2048 {
        let event = r.read_event().map_err(|_| ThemeError::Iterm)?;
        match event {
            Event::Start(e) => {
                let tag = e.name().as_ref().to_owned();
                if done || stack.len() >= 4 {
                    return Err(ThemeError::Iterm);
                }
                if stack.is_empty() {
                    if root || tag != "plist" {
                        return Err(ThemeError::Iterm);
                    }
                    root = true;
                    let attrs = e
                        .attributes()
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|_| ThemeError::Iterm)?;
                    if attrs.len() != 1
                        || attrs[0].key.as_ref() != "version"
                        || attrs[0].value.as_ref() != "1.0"
                    {
                        return Err(ThemeError::Iterm);
                    }
                } else {
                    if e.attributes().next().is_some() {
                        return Err(ThemeError::Iterm);
                    }
                    let parent = stack.last().ok_or(ThemeError::Iterm)?;
                    if parent == "plist" {
                        if tag != "dict" {
                            return Err(ThemeError::Iterm);
                        }
                    } else if parent != "dict"
                        || !matches!(tag.as_str(), "dict" | "key" | "string" | "real")
                    {
                        return Err(ThemeError::Iterm);
                    }
                }
                stack.push(tag);
            }
            Event::End(e) => {
                if stack.pop().as_deref() != Some(e.name().as_ref()) {
                    return Err(ThemeError::Iterm);
                }
                if stack.is_empty() {
                    done = true;
                }
            }
            Event::Text(t) => {
                if !stack
                    .last()
                    .is_some_and(|tag| matches!(tag.as_str(), "key" | "string" | "real"))
                    && !t.as_ref().as_bytes().iter().all(u8::is_ascii_whitespace)
                {
                    return Err(ThemeError::Iterm);
                }
            }
            Event::GeneralRef(e) => {
                if !stack
                    .last()
                    .is_some_and(|tag| matches!(tag.as_str(), "key" | "string" | "real"))
                    || (!matches!(e.as_ref(), "amp" | "lt" | "gt" | "quot" | "apos")
                        && e.resolve_char_ref()
                            .map_err(|_| ThemeError::Iterm)?
                            .is_none())
                {
                    return Err(ThemeError::Iterm);
                }
            }
            Event::DocType(e) => {
                if root || dtd || e.as_ref() != DTD {
                    return Err(ThemeError::Iterm);
                }
                dtd = true;
            }
            Event::Decl(e) => {
                if root || declaration {
                    return Err(ThemeError::Iterm);
                }
                if let Some(encoding) = e.encoding() {
                    let encoding = encoding.map_err(|_| ThemeError::Iterm)?;
                    if !encoding.as_ref().eq_ignore_ascii_case("UTF-8") {
                        return Err(ThemeError::Iterm);
                    }
                }
                declaration = true;
            }
            Event::Comment(_) => {}
            Event::Eof => {
                return if root && done && stack.is_empty() {
                    Ok(())
                } else {
                    Err(ThemeError::Iterm)
                };
            }
            _ => return Err(ThemeError::Iterm),
        }
    }
    Err(ThemeError::Iterm)
}
/// XML-only complete ANSI16 iTerm colors with explicitly tagged sRGB components.
/// Finite channels 0..1 are quantized to nearest RGB8 (not lossless float restoration).
/// Alpha must be absent or exactly 1. Unknown colors, color spaces and duplicate keys reject.
/// 64 KiB/4 XML levels/2048 events; canonical Apple DOCTYPE is inert, never fetched.
/// No external entities, binary plist, includes, user file reads, or UI application.
pub fn import_itermcolors(name: &str, text: &str) -> Result<TerminalScheme, ThemeError> {
    budget(text)?;
    validate_name(name)?;
    preflight(text)?;
    let Wire(mut colors) =
        plist::from_reader_xml(text.as_bytes()).map_err(|_| ThemeError::Iterm)?;
    let mut take = |key: &str| -> Result<Option<Rgb8>, ThemeError> {
        colors.remove(key).map(Components::rgb).transpose()
    };
    let foreground = take("Foreground Color")?.ok_or(ThemeError::Iterm)?;
    let background = take("Background Color")?.ok_or(ThemeError::Iterm)?;
    let mut ansi = [Rgb8::new(0, 0, 0); 16];
    for (i, c) in ansi.iter_mut().enumerate() {
        *c = take(&format!("Ansi {i} Color"))?.ok_or(ThemeError::Iterm)?;
    }
    let cursor = take("Cursor Color")?;
    let cursor_text = take("Cursor Text Color")?;
    let selection_background = take("Selection Color")?;
    let selection_foreground = take("Selected Text Color")?;
    if !colors.is_empty() {
        return Err(ThemeError::Iterm);
    }
    TerminalScheme::validated(
        name.to_owned(),
        TerminalPalette {
            foreground,
            background,
            black: ansi[0],
            red: ansi[1],
            green: ansi[2],
            yellow: ansi[3],
            blue: ansi[4],
            purple: ansi[5],
            cyan: ansi[6],
            white: ansi[7],
            bright_black: ansi[8],
            bright_red: ansi[9],
            bright_green: ansi[10],
            bright_yellow: ansi[11],
            bright_blue: ansi[12],
            bright_purple: ansi[13],
            bright_cyan: ansi[14],
            bright_white: ansi[15],
            cursor,
            cursor_text,
            selection_background,
            selection_foreground,
        },
    )
}
#[cfg(test)]
#[path = "theme_iterm_tests.rs"]
mod tests;
