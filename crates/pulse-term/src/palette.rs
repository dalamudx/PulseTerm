//! Caller-supplied terminal colors for authoritative protocol replies.
use alacritty_terminal::vte::ansi::Rgb;
/// Fallback palette, independent from semantic UI tokens. Remote OSC overrides
/// are owned by the terminal and take precedence until reset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPalette {
    /// Sixteen ANSI RGB triples in conventional order.
    pub ansi: [[u8; 3]; 16],
    /// Default text color.
    pub foreground: [u8; 3],
    /// Default terminal background.
    pub background: [u8; 3],
    /// Default cursor color.
    pub cursor: [u8; 3],
}
impl Default for QueryPalette {
    fn default() -> Self {
        Self {
            ansi: [
                [0, 0, 0],
                [205, 0, 0],
                [0, 205, 0],
                [205, 205, 0],
                [0, 0, 238],
                [205, 0, 205],
                [0, 205, 205],
                [229, 229, 229],
                [127, 127, 127],
                [255, 0, 0],
                [0, 255, 0],
                [255, 255, 0],
                [92, 92, 255],
                [255, 0, 255],
                [0, 255, 255],
                [255, 255, 255],
            ],
            foreground: [255, 255, 255],
            background: [0, 0, 0],
            cursor: [255, 255, 255],
        }
    }
}
impl QueryPalette {
    pub(crate) fn indexed(&self, index: usize) -> Option<Rgb> {
        let [r, g, b] = match index {
            0..=15 => self.ansi[index],
            16..=231 => {
                let cube = index - 16;
                let levels = [0, 95, 135, 175, 215, 255];
                [levels[cube / 36], levels[(cube / 6) % 6], levels[cube % 6]]
            }
            232..=255 => {
                let gray = (8 + 10 * (index - 232)) as u8;
                [gray; 3]
            }
            256 => self.foreground,
            257 => self.background,
            258 => self.cursor,
            _ => return None,
        };
        Some(Rgb { r, g, b })
    }
}
