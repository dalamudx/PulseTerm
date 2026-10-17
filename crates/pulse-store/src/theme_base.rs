use super::{
    Rgb8, TerminalPalette, TerminalScheme, ThemeAppearance, ThemeError, budget, validate_name,
};
use std::collections::BTreeMap;
use yaml_rust2::parser::{Event, Parser};
/// Explicit expected color system. Never inferred from a partially populated YAML document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseSystem {
    /// Tinted Base16 v0.4.2.
    Base16,
    /// Base24 v0.1.3.
    Base24,
}
impl BaseSystem {
    fn count(self) -> usize {
        match self {
            Self::Base16 => 16,
            Self::Base24 => 24,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Base16 => "base16",
            Self::Base24 => "base24",
        }
    }
}
/// Complete original base palette plus ANSI16 projection; extended ANSI16..21 are not applied.
/// Metadata is untrusted text, not paths, URLs, commands or template instructions.
pub struct BaseScheme {
    system: BaseSystem,
    colors: Vec<Rgb8>,
    terminal: TerminalScheme,
    variant: Option<ThemeAppearance>,
    metadata: BTreeMap<String, String>,
}
impl BaseScheme {
    /// Chosen and checked expected system.
    pub fn system(&self) -> BaseSystem {
        self.system
    }
    /// All 16 or 24 original colors in base00..base17 order, including colors outside ANSI16.
    pub fn source_palette(&self) -> &[Rgb8] {
        &self.colors
    }
    /// Standard ANSI16 projection, not a full base24 palette save operation.
    pub fn terminal(&self) -> &TerminalScheme {
        &self.terminal
    }
    /// Explicit recognized variant, if supplied; never inferred from luminance.
    pub fn variant(&self) -> Option<ThemeAppearance> {
        self.variant
    }
    /// Literal author/slug/description metadata. No network or filesystem use is authorized.
    pub fn metadata(&self) -> &BTreeMap<String, String> {
        &self.metadata
    }
}
struct Events<'a> {
    parser: Parser<std::str::Chars<'a>>,
    count: usize,
}
impl<'a> Events<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            parser: Parser::new(text.chars()),
            count: 0,
        }
    }
    fn next(&mut self) -> Result<Event, ThemeError> {
        self.count += 1;
        if self.count > 192 {
            return Err(ThemeError::Budget);
        }
        self.parser
            .next_token()
            .map(|(e, _)| e)
            .map_err(|_| ThemeError::BaseYaml)
    }
    fn scalar(e: Event) -> Result<String, ThemeError> {
        match e {
            Event::Scalar(s, _, 0, None) => Ok(s),
            _ => Err(ThemeError::BaseYaml),
        }
    }
    fn expect(&mut self, e: Event) -> Result<(), ThemeError> {
        if self.next()? == e {
            Ok(())
        } else {
            Err(ThemeError::BaseYaml)
        }
    }
}
fn color(s: String) -> Result<Rgb8, ThemeError> {
    let text = if s.len() == 6 {
        format!("#{s}")
    } else if s.len() == 7 {
        s
    } else {
        return Err(ThemeError::Color);
    };
    Rgb8::try_from(text)
}
fn index(key: &str, expected: BaseSystem) -> Result<usize, ThemeError> {
    if key.len() != 6 || !key.starts_with("base") {
        return Err(ThemeError::BaseYaml);
    }
    let n = usize::from_str_radix(&key[4..], 16).map_err(|_| ThemeError::BaseYaml)?;
    if n >= expected.count() || format!("base{n:02X}") != key {
        return Err(ThemeError::BaseYaml);
    }
    Ok(n)
}
fn insert(
    colors: &mut [Option<Rgb8>; 24],
    key: String,
    value: String,
    expected: BaseSystem,
) -> Result<(), ThemeError> {
    let n = index(&key, expected)?;
    if colors[n].is_some() {
        return Err(ThemeError::BaseYaml);
    }
    colors[n] = Some(color(value)?);
    Ok(())
}
/// Import current common or legacy scheme YAML, rejecting tags/anchors/aliases, duplicates,
/// sequences, non-scalar metadata, mixed layouts, multiple documents and extra bases.
/// Bounded 64KiB, max192 events, max2 maps deep; no tree loader/template engine is invoked.
pub fn import_base_scheme(text: &str, expected: BaseSystem) -> Result<BaseScheme, ThemeError> {
    budget(text)?;
    let mut e = Events::new(text);
    e.expect(Event::StreamStart)?;
    e.expect(Event::DocumentStart)?;
    e.expect(Event::MappingStart(0, None))?;
    let mut fields = BTreeMap::new();
    let mut colors = [None; 24];
    let mut nested = false;
    loop {
        let key = match e.next()? {
            Event::MappingEnd => break,
            event => Events::scalar(event)?,
        };
        if key.len() > 32 || fields.contains_key(&key) {
            return Err(ThemeError::BaseYaml);
        }
        if key == "palette" {
            nested = true;
            fields.insert(key, String::new());
            e.expect(Event::MappingStart(0, None))?;
            loop {
                let key = match e.next()? {
                    Event::MappingEnd => break,
                    event => Events::scalar(event)?,
                };
                let value = Events::scalar(e.next()?)?;
                insert(&mut colors, key, value, expected)?;
            }
            continue;
        }
        if key.starts_with("base") {
            let value = Events::scalar(e.next()?)?;
            insert(&mut colors, key.clone(), value, expected)?;
            fields.insert(key, String::new());
            continue;
        }
        if !matches!(
            key.as_str(),
            "name" | "scheme" | "system" | "author" | "description" | "slug" | "variant"
        ) {
            return Err(ThemeError::BaseYaml);
        }
        let value = Events::scalar(e.next()?)?;
        if value.len() > 4096 || value.chars().any(|c| c == '\0') {
            return Err(ThemeError::Budget);
        }
        fields.insert(key, value);
    }
    e.expect(Event::DocumentEnd)?;
    e.expect(Event::StreamEnd)?;
    let name = if nested {
        if fields.contains_key("scheme")
            || fields.keys().any(|key| key.starts_with("base"))
            || fields.get("system").map(String::as_str) != Some(expected.name())
        {
            return Err(ThemeError::BaseYaml);
        }
        fields.remove("name").ok_or(ThemeError::BaseYaml)?
    } else {
        if fields.contains_key("name")
            || fields.contains_key("system")
            || fields.contains_key("variant")
        {
            return Err(ThemeError::BaseYaml);
        }
        fields.remove("scheme").ok_or(ThemeError::BaseYaml)?
    };
    validate_name(&name)?;
    let variant = match fields.remove("variant").as_deref() {
        None => None,
        Some("light") => Some(ThemeAppearance::Light),
        Some("dark") => Some(ThemeAppearance::Dark),
        _ => return Err(ThemeError::BaseYaml),
    };
    if fields.get("author").is_none_or(|s| s.trim().is_empty()) {
        return Err(ThemeError::BaseYaml);
    }
    let colors: Vec<Rgb8> = colors[..expected.count()]
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()
        .ok_or(ThemeError::BaseYaml)?;
    let mapping = match expected {
        BaseSystem::Base16 => [0, 8, 11, 10, 13, 14, 12, 5, 3, 8, 11, 10, 13, 14, 12, 7],
        BaseSystem::Base24 => [0, 8, 11, 10, 13, 14, 12, 5, 3, 18, 20, 19, 22, 23, 21, 7],
    };
    let names = [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "purple",
        "cyan",
        "white",
        "brightBlack",
        "brightRed",
        "brightGreen",
        "brightYellow",
        "brightBlue",
        "brightPurple",
        "brightCyan",
        "brightWhite",
    ];
    let mut wire = serde_json::Map::new();
    for (key, i) in names.into_iter().zip(mapping) {
        wire.insert(
            key.into(),
            serde_json::to_value(colors[i]).map_err(|_| ThemeError::Encoding)?,
        );
    }
    for (key, i) in [
        ("foreground", 5),
        ("background", 0),
        ("cursor", 5),
        ("selectionBackground", 2),
        ("selectionForeground", 5),
    ] {
        wire.insert(
            key.into(),
            serde_json::to_value(colors[i]).map_err(|_| ThemeError::Encoding)?,
        );
    }
    let terminal: TerminalPalette = serde_json::from_value(serde_json::Value::Object(wire))
        .map_err(|_| ThemeError::Encoding)?;
    fields.retain(|key, _| matches!(key.as_str(), "author" | "slug" | "description"));
    Ok(BaseScheme {
        system: expected,
        colors,
        terminal: TerminalScheme { name, terminal },
        variant,
        metadata: fields,
    })
}
#[cfg(test)]
#[path = "theme_base_tests.rs"]
mod tests;
