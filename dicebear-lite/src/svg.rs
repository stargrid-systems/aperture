//! SVG rendering helpers: XML escaping and CSS function types.

use core::fmt::{self, Write};

#[expect(
    unused_imports,
    reason = "Write trait needed for write_str method resolution"
)]
use Write as _;

use crate::number::Num;

/// A string wrapper that escapes the five significant XML characters when
/// formatted.
pub struct Escaped<'a>(pub &'a str);

impl fmt::Display for Escaped<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for ch in self.0.chars() {
            match ch {
                '&' => f.write_str("&amp;")?,
                '\'' => f.write_str("&apos;")?,
                '"' => f.write_str("&quot;")?,
                '<' => f.write_str("&lt;")?,
                '>' => f.write_str("&gt;")?,
                other => f.write_str(other.encode_utf8(&mut [0u8; 4]))?,
            }
        }
        Ok(())
    }
}

/// CSS `translate(x, y)`.
pub struct Translate(pub f64, pub f64);
impl fmt::Display for Translate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "translate({x}, {y})", x = Num(self.0), y = Num(self.1))
    }
}

/// CSS `rotate(angle, cx, cy)`.
pub struct Rotate(pub f64, pub f64, pub f64);
impl fmt::Display for Rotate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rotate({a}, {cx}, {cy})",
            a = Num(self.0),
            cx = Num(self.1),
            cy = Num(self.2)
        )
    }
}

/// CSS `scale(s)`.
pub struct Scale(pub f64);
impl fmt::Display for Scale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scale({s})", s = Num(self.0))
    }
}

/// CSS `translate(cx, cy) scale(s) translate(-cx, -cy)`.
pub struct ScaleAtPoint {
    pub scale: f64,
    pub cx: f64,
    pub cy: f64,
}
impl fmt::Display for ScaleAtPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{a} {b} {c}",
            a = Translate(self.cx, self.cy),
            b = Scale(self.scale),
            c = Translate(-self.cx, -self.cy)
        )
    }
}

/// SVG `<use>` element with an optional transform.
pub struct UseElement<'a> {
    pub source: &'a str,
    pub variant_name: &'a str,
    pub hash: u32,
    pub user_transform: Option<&'a str>,
    pub translate: Option<(f64, f64)>,
    pub rotate: Option<(f64, f64, f64)>,
    pub scale: Option<ScaleAtPoint>,
}

impl fmt::Display for UseElement<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<use")?;
        let has_transform = self.user_transform.is_some()
            || self.translate.is_some()
            || self.rotate.is_some()
            || self.scale.is_some();
        if has_transform {
            f.write_str(" transform=\"")?;
            let mut wrote = if let Some(ut) = self.user_transform {
                f.write_str(ut)?;
                true
            } else {
                false
            };
            if let Some((tx, ty)) = self.translate {
                if wrote {
                    f.write_str(" ")?;
                }
                write!(f, "{t}", t = Translate(tx, ty))?;
                wrote = true;
            }
            if let Some((a, cx, cy)) = self.rotate {
                if wrote {
                    f.write_str(" ")?;
                }
                write!(f, "{r}", r = Rotate(a, cx, cy))?;
                wrote = true;
            }
            if let Some(ref s) = self.scale {
                if wrote {
                    f.write_str(" ")?;
                }
                write!(f, "{s}")?;
            }
            f.write_str("\"")?;
        }
        write!(
            f,
            " href=\"#{source}-{variant_name}-{hash:08x}\"/>",
            source = self.source,
            variant_name = self.variant_name,
            hash = self.hash
        )
    }
}
