#[doc(hidden)]
struct GammaValues {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Gamma correction lookup table.
pub struct GammaLookup {
    #[doc(hidden)]
    table: Vec<GammaValues>,
}

impl GammaLookup {
    /// Create a new GammaLookup instance to perform gamma correction on the RGB
    /// channels for each LED color.
    pub fn new() -> Self {
        Self {
            table: (0_u8..255)
                .map(|index| {
                    let f = ((index as f64) / 255.0).powf(2.8);
                    GammaValues {
                        r: (f * 255.0) as u8,
                        g: (f * 240.0) as u8,
                        b: (f * 220.0) as u8,
                    }
                })
                .collect(),
        }
    }

    /// Get a gamma corrected value for the red channel.
    pub fn red(&self, r: u8) -> u8 {
        self.table[usize::from(r)].r
    }

    /// Get a gamma corrected value for the green channel.
    pub fn green(&self, g: u8) -> u8 {
        self.table[usize::from(g)].g
    }

    /// Get a gamma corrected value for the blue channel.
    pub fn blue(&self, b: u8) -> u8 {
        self.table[usize::from(b)].b
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn new_gamma_lookup() -> () {
        let gamma_lookup = GammaLookup::new();
        assert_eq!(gamma_lookup.table.len(), 255);
    }
}