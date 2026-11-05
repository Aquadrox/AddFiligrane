//! Transformations affines au format PDF (`[a b c d e f]`, vecteurs lignes).

/// Transformation affine `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    #[cfg(test)]
    pub const IDENTITY: Self = Self::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);

    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Self { a, b, c, d, e, f }
    }

    pub const fn translate(x: f64, y: f64) -> Self {
        Self::new(1.0, 0.0, 0.0, 1.0, x, y)
    }

    pub const fn scale(sx: f64, sy: f64) -> Self {
        Self::new(sx, 0.0, 0.0, sy, 0.0, 0.0)
    }

    /// Rotation d'angle `radians`, sens trigonométrique.
    pub fn rotate(radians: f64) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self::new(cos, sin, -sin, cos, 0.0, 0.0)
    }

    /// Composition : applique `self`, puis `next`.
    pub fn then(&self, next: &Self) -> Self {
        Self::new(
            self.a * next.a + self.b * next.c,
            self.a * next.b + self.b * next.d,
            self.c * next.a + self.d * next.c,
            self.c * next.b + self.d * next.d,
            self.e * next.a + self.f * next.c + next.e,
            self.e * next.b + self.f * next.d + next.f,
        )
    }

    #[cfg(test)]
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    /// Opérande de l'opérateur PDF `cm`.
    pub fn to_pdf_operands(self) -> String {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .map(format_number)
            .join(" ")
    }
}

/// Rectangle aligné sur les axes, normalisé (`x0 <= x1`, `y0 <= y1`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    pub fn new(xa: f64, ya: f64, xb: f64, yb: f64) -> Self {
        Self {
            x0: xa.min(xb),
            y0: ya.min(yb),
            x1: xa.max(xb),
            y1: ya.max(yb),
        }
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    /// Intersection, ou `None` si elle est vide.
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        let rect = Self {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        };
        (rect.width() > 0.0 && rect.height() > 0.0).then_some(rect)
    }
}

/// Nombre PDF compact : 4 décimales au plus, sans zéros superflus.
pub fn format_number(value: f64) -> String {
    let value = if value.abs() < 5e-5 { 0.0 } else { value };
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "-0" => "0".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    fn assert_close(actual: (f64, f64), expected: (f64, f64)) {
        assert!(
            (actual.0 - expected.0).abs() < 1e-9 && (actual.1 - expected.1).abs() < 1e-9,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn then_applies_left_operand_first() {
        let m = Affine::scale(2.0, 3.0).then(&Affine::translate(10.0, 20.0));
        assert_close(m.apply(1.0, 1.0), (12.0, 23.0));
        let m = Affine::translate(10.0, 20.0).then(&Affine::scale(2.0, 3.0));
        assert_close(m.apply(1.0, 1.0), (22.0, 63.0));
    }

    #[test]
    fn rotation_is_counter_clockwise() {
        assert_close(Affine::rotate(FRAC_PI_2).apply(1.0, 0.0), (0.0, 1.0));
    }

    #[test]
    fn numbers_are_compact() {
        assert_eq!(format_number(1.0), "1");
        assert_eq!(format_number(-0.00001), "0");
        assert_eq!(format_number(12.345_67), "12.3457");
        assert_eq!(format_number(-3.5), "-3.5");
        assert_eq!(Affine::IDENTITY.to_pdf_operands(), "1 0 0 1 0 0");
    }

    #[test]
    fn rect_is_normalized_and_intersects() {
        let a = Rect::new(10.0, 10.0, 0.0, 0.0);
        assert_eq!((a.x0, a.y1), (0.0, 10.0));
        let b = Rect::new(5.0, 5.0, 20.0, 20.0);
        assert_eq!(a.intersect(&b), Some(Rect::new(5.0, 5.0, 10.0, 10.0)));
        assert_eq!(a.intersect(&Rect::new(11.0, 0.0, 12.0, 1.0)), None);
    }
}
