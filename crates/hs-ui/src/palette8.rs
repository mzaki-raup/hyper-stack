//! The rotating palette of eight (CR-0059, moved here by CR-0082): mid-tone
//! colours that read on the dark and the light theme alike, ordered so two
//! neighbours never look alike. Logs' saved sources take one each by
//! position; a workspace's lettered tile takes one by its name.

use egui::Color32;

/// Blue, orange, emerald, violet, gold, cyan, rose, fuchsia.
pub const COLOURS: [Color32; 8] = [
    Color32::from_rgb(0x3b, 0x82, 0xf6),
    Color32::from_rgb(0xea, 0x58, 0x0c),
    Color32::from_rgb(0x10, 0xb9, 0x81),
    Color32::from_rgb(0x8b, 0x5c, 0xf6),
    Color32::from_rgb(0xca, 0x8a, 0x04),
    Color32::from_rgb(0x06, 0xb6, 0xd4),
    Color32::from_rgb(0xf4, 0x3f, 0x5e),
    Color32::from_rgb(0xd9, 0x46, 0xef),
];

/// The colour at `position`, round again after eight.
pub fn by_index(position: usize) -> Color32 {
    COLOURS[position % COLOURS.len()]
}

/// The colour for `name`: a stable hash of it (32-bit FNV-1a), so the same
/// name wears the same colour after a restart and on every machine.
pub fn by_name(name: &str) -> Color32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in name.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    by_index(hash as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_keeps_its_colour_and_it_is_one_of_the_eight() {
        for name in ["default", "hyper-stack", "My Work Space", "", "x"] {
            let c = by_name(name);
            assert_eq!(c, by_name(name));
            assert!(COLOURS.contains(&c), "{name}");
        }
        // FNV-1a of "" is its offset basis, 0x811c9dc5, which is 5 mod 8.
        assert_eq!(by_name(""), COLOURS[5]);
    }

    #[test]
    fn the_index_rotates_after_eight() {
        for i in 0..32 {
            assert_eq!(by_index(i), by_index(i + COLOURS.len()));
        }
    }

    /// Two saved items side by side never wear the same colour, at any
    /// place in the list (CR-0059).
    #[test]
    fn two_neighbours_never_share_a_colour() {
        for i in 0..64 {
            assert_ne!(by_index(i), by_index(i + 1), "items {i} and {}", i + 1);
        }
    }
}
