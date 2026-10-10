#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    pub const BLACK: Self = Self::new(0, 0, 0);
    pub const WHITE: Self = Self::new(255, 255, 255);

    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub fn mix(self, other: Self, amount: f32) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        let mix_channel =
            |a: u8, b: u8| ((a as f32 * (1.0 - amount)) + (b as f32 * amount)).round() as u8;

        Self::new(
            mix_channel(self.red, other.red),
            mix_channel(self.green, other.green),
            mix_channel(self.blue, other.blue),
        )
    }

    pub fn contrast_against(self, other: Self) -> f64 {
        contrast_ratio(self, other)
    }

    pub fn best_foreground(self) -> Self {
        let white_contrast = contrast_ratio(self, Self::WHITE);
        let black_contrast = contrast_ratio(self, Self::BLACK);

        if black_contrast >= white_contrast {
            Self::BLACK
        } else {
            Self::WHITE
        }
    }
}

fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let lighter = relative_luminance(a).max(relative_luminance(b));
    let darker = relative_luminance(a).min(relative_luminance(b));
    (lighter + 0.05) / (darker + 0.05)
}

fn relative_luminance(color: Rgb) -> f64 {
    fn channel(value: u8) -> f64 {
        let value = f64::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    (0.2126 * channel(color.red)) + (0.7152 * channel(color.green)) + (0.0722 * channel(color.blue))
}

#[cfg(test)]
mod tests {
    use super::Rgb;

    #[test]
    fn bright_accents_choose_dark_text() {
        assert_eq!(Rgb::new(255, 220, 0).best_foreground(), Rgb::BLACK);
    }

    #[test]
    fn dark_accents_choose_light_text() {
        assert_eq!(Rgb::new(25, 70, 150).best_foreground(), Rgb::WHITE);
    }

    #[test]
    fn mixing_uses_both_endpoints() {
        let mixed = Rgb::BLACK.mix(Rgb::WHITE, 0.5);
        assert_eq!(mixed, Rgb::new(128, 128, 128));
    }
}
