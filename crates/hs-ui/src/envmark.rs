//! An environment's mark: the icon and colour of its tier (FEAT-0106,
//! docs/04 "Environment marks"). A protected environment always wears the
//! red shield, whatever its tier.

use egui::Color32;
use egui_phosphor::regular as ph;
use hs_core::environments::{Environment, Tier};

use crate::theme::Theme;

/// The glyph and colour for an environment that is `protected` and of
/// `tier` (its effective tier: set by hand, else matched from its name).
pub fn mark(protected: bool, tier: Option<Tier>, theme: &Theme) -> (&'static str, Color32) {
    if protected {
        return (ph::SHIELD_WARNING, theme.error);
    }
    match tier {
        Some(Tier::Local) => (ph::LAPTOP, theme.text_muted),
        Some(Tier::Development) => (ph::CODE, theme.env_dev),
        Some(Tier::Integration) => (ph::PLUGS_CONNECTED, Color32::from_rgb(0x2A, 0xA1, 0x98)),
        Some(Tier::Testing) => (ph::FLASK, Color32::from_rgb(0x9B, 0x6B, 0xDF)),
        Some(Tier::Training) => (ph::GRADUATION_CAP, theme.success),
        Some(Tier::Acceptance) => (ph::CHECK_SQUARE, theme.warning),
        Some(Tier::Staging) => (ph::STACK, Color32::from_rgb(0xE8, 0x84, 0x2A)),
        Some(Tier::ProductionSupport) => (ph::LIFEBUOY, Color32::from_rgb(0xE5, 0x6A, 0x2E)),
        Some(Tier::Production) => (ph::SHIELD_WARNING, theme.env_prod),
        None => (ph::GLOBE, theme.text_muted),
    }
}

/// [`mark`] for a saved environment.
pub fn env_mark(env: &Environment, theme: &Theme) -> (&'static str, Color32) {
    mark(env.protected, env.effective_tier(), theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_always_wears_the_shield() {
        let theme = Theme::dark();
        let mut tiers: Vec<Option<Tier>> = Tier::ALL.iter().copied().map(Some).collect();
        tiers.push(None);
        for tier in tiers {
            assert_eq!(
                mark(true, tier, &theme),
                (ph::SHIELD_WARNING, theme.error),
                "{tier:?}"
            );
        }
        assert_eq!(mark(false, None, &theme).0, ph::GLOBE);
        assert_eq!(mark(false, Some(Tier::Local), &theme).0, ph::LAPTOP);
    }
}
