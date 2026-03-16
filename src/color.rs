#[derive(Copy, Clone, Debug)]
pub struct Color {
    pub red: u32,
    pub green: u32,
    pub blue: u32,
    pub alpha: u32,
}

impl Color {
    pub fn from_rgb(r: u32, g: u32, b: u32) -> Color {
        Color {
            red: r,
            green: g,
            blue: b,
            alpha: 255,
        }
    }

    pub fn from_rgba(r: u32, g: u32, b: u32, a: u32) -> Color {
        Color {
            red: r,
            green: g,
            blue: b,
            alpha: 255,
        }
    }

    pub fn from_hex(hex: u32) -> Color {
        Color {
            red: ((hex >> 16) & 0xFF) as u32,
            green: ((hex >> 8) & 0xFF) as u32,
            blue: (hex & 0xFF) as u32,
            alpha: 255,
        }
    }

    pub fn red() -> Color {
        Color::from_rgb(255, 0, 0)
    }

    pub fn green() -> Color {
        Color::from_rgb(0, 255, 0)
    }

    pub fn blue() -> Color {
        Color::from_rgb(0, 0, 255)
    }

    pub fn to_hex_rgb(&self) -> u32 {
        (self.red << 16) | (self.green << 8) | self.blue
    }

    pub fn to_hex_rgba(&self) -> u32 {
        (self.red << 16) | (self.green << 8) | self.blue | self.alpha
    }
}
