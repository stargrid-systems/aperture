//! Runtime-configurable avatar settings: style and animation.
//!
//! [`AvatarStyle`] and [`AvatarAnimation`] are [`SettingDefinition`]s. Their
//! values are read per request by the avatar handler and mapped onto the
//! corresponding `dicebear-lite` types, so the gateway's avatars can be
//! reconfigured without a restart.

use aperture_settings::SettingDefinition;
use dicebear_lite::Animation;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Avatar style setting, mapped to a `dicebear-lite` style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AvatarStyle {
    #[default]
    Constellation,
    Planets,
    Thumbs,
}

impl AvatarStyle {
    /// The variant name as it appears in an `ETag` (`"constellation"`, ...).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Constellation => "constellation",
            Self::Planets => "planets",
            Self::Thumbs => "thumbs",
        }
    }

    /// Maps this setting to the corresponding `dicebear-lite` style.
    #[must_use]
    pub const fn style(self) -> &'static dicebear_lite::Style<'static> {
        match self {
            Self::Constellation => &dicebear_lite::CONSTELLATION,
            Self::Planets => &dicebear_lite::PLANETS,
            Self::Thumbs => &dicebear_lite::THUMBS,
        }
    }
}

impl SettingDefinition for AvatarStyle {
    const KEY: &'static str = "avatar_style";
}

/// Avatar animation setting, mapped to `dicebear_lite::Animation`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AvatarAnimation {
    #[default]
    Off,
    Random,
    Fastest,
    Fast,
    Medium,
    Slow,
    Slowest,
}

impl AvatarAnimation {
    /// The variant name as it appears in an `ETag` (`"off"`, `"random"`, ...).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Random => "random",
            Self::Fastest => "fastest",
            Self::Fast => "fast",
            Self::Medium => "medium",
            Self::Slow => "slow",
            Self::Slowest => "slowest",
        }
    }

    /// Maps this setting to the corresponding `dicebear-lite` animation.
    #[must_use]
    pub const fn animation(self) -> Animation {
        match self {
            Self::Off => Animation::Off,
            Self::Random => Animation::Random,
            Self::Fastest => Animation::Fixed(dicebear_lite::Speed::Fastest),
            Self::Fast => Animation::Fixed(dicebear_lite::Speed::Fast),
            Self::Medium => Animation::Fixed(dicebear_lite::Speed::Medium),
            Self::Slow => Animation::Fixed(dicebear_lite::Speed::Slow),
            Self::Slowest => Animation::Fixed(dicebear_lite::Speed::Slowest),
        }
    }
}

impl SettingDefinition for AvatarAnimation {
    const KEY: &'static str = "avatar_animation";
}
