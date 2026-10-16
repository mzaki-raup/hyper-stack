//! Generators: identifiers, numbers, passwords and passphrases.
//!
//! Everything here is a plain function over its inputs so the behaviour can
//! be tested without a UI, and so the one place that decides what a
//! "strong" password means is visible rather than scattered through the
//! widget code.

use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;

// ---- identifiers ---------------------------------------------------------

/// Which flavour of UUID to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UuidKind {
    /// Version 4: 122 random bits. What most code means by "a UUID".
    #[default]
    V4,
    /// Version 7: a millisecond timestamp then random bits, so ids sort in
    /// the order they were made — which matters for database keys.
    V7,
    /// The nil UUID, all zeroes.
    Nil,
}

/// A UUID of that kind, in the usual hyphenated form.
///
/// `now_ms` is only read for [`UuidKind::V7`]; passing it in rather than
/// reading the clock keeps the function testable.
pub fn uuid(kind: UuidKind, rng: &mut impl Rng, now_ms: u64) -> String {
    let mut b = [0u8; 16];
    rng.fill(&mut b);
    match kind {
        UuidKind::Nil => return format_uuid(&[0u8; 16]),
        UuidKind::V4 => {
            b[6] = (b[6] & 0x0F) | 0x40;
        }
        UuidKind::V7 => {
            let ms = now_ms & 0x0000_FFFF_FFFF_FFFF;
            b[0] = (ms >> 40) as u8;
            b[1] = (ms >> 32) as u8;
            b[2] = (ms >> 24) as u8;
            b[3] = (ms >> 16) as u8;
            b[4] = (ms >> 8) as u8;
            b[5] = ms as u8;
            b[6] = (b[6] & 0x0F) | 0x70;
        }
    }
    // The variant bits are the same for every version that has them.
    b[8] = (b[8] & 0x3F) | 0x80;
    format_uuid(&b)
}

fn format_uuid(b: &[u8; 16]) -> String {
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

// ---- numbers -------------------------------------------------------------

/// `count` numbers in `low..=high`.
///
/// A seed makes the run repeatable, which is the point of asking for one:
/// the same seed and bounds always give the same numbers. Bounds the wrong
/// way round are swapped rather than refused.
pub fn numbers(low: i64, high: i64, count: usize, seed: Option<u64>) -> Vec<i64> {
    let (low, high) = if low <= high {
        (low, high)
    } else {
        (high, low)
    };
    let count = count.min(10_000);
    let mut rng = seeded(seed);
    (0..count).map(|_| rng.random_range(low..=high)).collect()
}

fn seeded(seed: Option<u64>) -> StdRng {
    match seed {
        Some(s) => StdRng::seed_from_u64(s),
        None => StdRng::from_rng(&mut rand::rng()),
    }
}

// ---- passwords -----------------------------------------------------------

/// What a generated password must be made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordRules {
    pub length: usize,
    pub lower: bool,
    pub upper: bool,
    pub digits: bool,
    pub symbols: bool,
    /// Leave out characters that are hard to tell apart when read aloud or
    /// off a screen: O/0, l/1/I and friends.
    pub avoid_ambiguous: bool,
}

impl Default for PasswordRules {
    fn default() -> Self {
        Self {
            length: 20,
            lower: true,
            upper: true,
            digits: true,
            symbols: true,
            avoid_ambiguous: false,
        }
    }
}

const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const SYMBOLS: &str = "!#$%&()*+-:;<=>?@[]^_{|}~";
const AMBIGUOUS: &str = "O0oIl1|S5B8G6Z2";

/// A password following `rules`, or the reason it cannot be made.
///
/// Every chosen set is guaranteed to appear at least once — a rule that
/// says "include digits" and then produces none is a rule that lied — and
/// the result is shuffled afterwards so those characters are not always in
/// the same places.
pub fn password(rules: &PasswordRules, seed: Option<u64>) -> Result<String, String> {
    let mut sets: Vec<Vec<char>> = Vec::new();
    for (on, chars) in [
        (rules.lower, LOWER),
        (rules.upper, UPPER),
        (rules.digits, DIGITS),
        (rules.symbols, SYMBOLS),
    ] {
        if !on {
            continue;
        }
        let set: Vec<char> = chars
            .chars()
            .filter(|c| !(rules.avoid_ambiguous && AMBIGUOUS.contains(*c)))
            .collect();
        if !set.is_empty() {
            sets.push(set);
        }
    }
    if sets.is_empty() {
        return Err("choose at least one kind of character".into());
    }
    if rules.length < sets.len() {
        return Err(format!(
            "a password of {} needs at least {} characters to hold one of each kind",
            rules.length,
            sets.len()
        ));
    }
    if rules.length > 512 {
        return Err("512 characters is the longest this makes".into());
    }
    let mut rng = seeded(seed);
    let all: Vec<char> = sets.iter().flatten().copied().collect();
    let mut out: Vec<char> = sets
        .iter()
        .map(|set| set[rng.random_range(0..set.len())])
        .collect();
    while out.len() < rules.length {
        out.push(all[rng.random_range(0..all.len())]);
    }
    // Fisher-Yates, so the guaranteed characters are not always first.
    for i in (1..out.len()).rev() {
        out.swap(i, rng.random_range(0..=i));
    }
    Ok(out.into_iter().collect())
}

/// How many bits of entropy a password drawn from `rules` carries.
pub fn password_entropy(rules: &PasswordRules) -> f64 {
    let mut pool = 0usize;
    for (on, chars) in [
        (rules.lower, LOWER),
        (rules.upper, UPPER),
        (rules.digits, DIGITS),
        (rules.symbols, SYMBOLS),
    ] {
        if on {
            pool += chars
                .chars()
                .filter(|c| !(rules.avoid_ambiguous && AMBIGUOUS.contains(*c)))
                .count();
        }
    }
    if pool < 2 {
        return 0.0;
    }
    rules.length as f64 * (pool as f64).log2()
}

// ---- passphrases ---------------------------------------------------------

/// The words a passphrase is drawn from.
///
/// Written here rather than taken from a published list so there is no
/// licence attached to it. It is deliberately short and plain: every word
/// is easy to spell and to say over a phone. The count is what the entropy
/// calculation uses, so it must never be guessed — see
/// [`passphrase_entropy`].
pub const WORDS: &[&str] = &[
    "amber",
    "anchor",
    "apple",
    "arrow",
    "autumn",
    "bacon",
    "badger",
    "bamboo",
    "banjo",
    "barley",
    "basin",
    "basket",
    "beacon",
    "beetle",
    "bench",
    "birch",
    "biscuit",
    "bishop",
    "blanket",
    "blossom",
    "bottle",
    "boulder",
    "bracket",
    "branch",
    "bridge",
    "bronze",
    "bucket",
    "bundle",
    "burrow",
    "button",
    "cabin",
    "cactus",
    "camel",
    "candle",
    "canvas",
    "canyon",
    "carpet",
    "carrot",
    "castle",
    "cedar",
    "cellar",
    "cement",
    "chapel",
    "cherry",
    "chimney",
    "cinder",
    "circle",
    "cliff",
    "clover",
    "cobalt",
    "collar",
    "comet",
    "compass",
    "copper",
    "coral",
    "cotton",
    "cradle",
    "crater",
    "crayon",
    "cricket",
    "crystal",
    "cupboard",
    "curtain",
    "cushion",
    "daisy",
    "dagger",
    "damson",
    "dangle",
    "dapple",
    "dawn",
    "deacon",
    "desert",
    "diamond",
    "dolphin",
    "domino",
    "donkey",
    "dragon",
    "drawer",
    "driver",
    "dumpling",
    "eagle",
    "ember",
    "emerald",
    "engine",
    "envelope",
    "fabric",
    "falcon",
    "fathom",
    "feather",
    "fennel",
    "ferry",
    "fiddle",
    "filter",
    "finch",
    "flagon",
    "flannel",
    "flask",
    "flint",
    "flower",
    "forest",
    "fossil",
    "fountain",
    "fragment",
    "frost",
    "garden",
    "garlic",
    "gazelle",
    "ginger",
    "glacier",
    "glider",
    "granite",
    "grape",
    "gravel",
    "guitar",
    "hammer",
    "harbour",
    "harvest",
    "hazel",
    "heather",
    "hedge",
    "helmet",
    "hickory",
    "hollow",
    "honey",
    "hornet",
    "island",
    "ivory",
    "jacket",
    "jasmine",
    "jigsaw",
    "jungle",
    "juniper",
    "kettle",
    "kitten",
    "lantern",
    "lattice",
    "lavender",
    "ledger",
    "lemon",
    "lentil",
    "lighthouse",
    "lilac",
    "linen",
    "lobster",
    "locket",
    "lumber",
    "magnet",
    "mallet",
    "mammoth",
    "mandolin",
    "maple",
    "marble",
    "marigold",
    "meadow",
    "melon",
    "mercury",
    "mineral",
    "mitten",
    "monsoon",
    "morning",
    "mosaic",
    "mustard",
    "nectar",
    "needle",
    "nettle",
    "nimble",
    "nutmeg",
    "oasis",
    "oatmeal",
    "obsidian",
    "orchard",
    "orchid",
    "otter",
    "outpost",
    "oyster",
    "paddle",
    "pantry",
    "parcel",
    "parsley",
    "pasture",
    "pebble",
    "pelican",
    "pepper",
    "petal",
    "pewter",
    "pigeon",
    "pillar",
    "pistachio",
    "planet",
    "plaster",
    "plover",
    "plum",
    "pocket",
    "pollen",
    "pony",
    "poplar",
    "poppy",
    "portal",
    "possum",
    "pottery",
    "prairie",
    "pretzel",
    "pudding",
    "pumpkin",
    "quarry",
    "quartz",
    "quiver",
    "radish",
    "rafter",
    "rapid",
    "rattle",
    "raven",
    "ribbon",
    "ridge",
    "rifle",
    "ripple",
    "river",
    "robin",
    "rocket",
    "rooster",
    "rosemary",
    "rudder",
    "saffron",
    "sailor",
    "salmon",
    "sandal",
    "sapphire",
    "satchel",
    "saucer",
    "scarlet",
    "scissor",
    "sepia",
    "shadow",
    "shamrock",
    "shelter",
    "shingle",
    "shovel",
    "shutter",
    "silver",
    "siren",
    "sketch",
    "slipper",
    "smoke",
    "snapper",
    "socket",
    "sorrel",
    "spanner",
    "sparrow",
    "spindle",
    "spruce",
    "squash",
    "stable",
    "stamp",
    "starling",
    "statue",
    "stencil",
    "stirrup",
    "stormy",
    "stubble",
    "sugar",
    "summit",
    "sunset",
    "swallow",
    "sweater",
    "sycamore",
    "table",
    "tadpole",
    "tangle",
    "tapestry",
    "teapot",
    "temple",
    "tender",
    "thimble",
    "thistle",
    "thunder",
    "timber",
    "tinder",
    "toffee",
    "token",
    "tomato",
    "topaz",
    "torch",
    "tortoise",
    "tower",
    "tractor",
    "trellis",
    "triangle",
    "trolley",
    "trumpet",
    "tulip",
    "tunnel",
    "turnip",
    "turtle",
    "umbrella",
    "unicorn",
    "valley",
    "vanilla",
    "velvet",
    "vendor",
    "vessel",
    "village",
    "vinegar",
    "violet",
    "walnut",
    "walrus",
    "wander",
    "warden",
    "wagon",
    "wheat",
    "whisker",
    "willow",
    "window",
    "winter",
    "wisteria",
    "wombat",
    "wooden",
    "yarrow",
    "yellow",
    "yonder",
    "zebra",
    "zenith",
    "zephyr",
    "zigzag",
];

/// How a passphrase's words are joined and shaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassphraseRules {
    pub words: usize,
    pub separator: String,
    /// Capitalise each word.
    pub capitalise: bool,
    /// Put a digit on the end, for the sites that insist on one.
    pub add_number: bool,
}

impl Default for PassphraseRules {
    fn default() -> Self {
        Self {
            // Six, not five: each word is worth about 8.3 bits from this
            // list, so five would be a weak default.
            words: 6,
            separator: "-".into(),
            capitalise: false,
            add_number: false,
        }
    }
}

/// A passphrase of `rules.words` words joined by the separator.
pub fn passphrase(rules: &PassphraseRules, seed: Option<u64>) -> Result<String, String> {
    if rules.words == 0 {
        return Err("a passphrase needs at least one word".into());
    }
    if rules.words > 64 {
        return Err("64 words is the longest this makes".into());
    }
    let mut rng = seeded(seed);
    let mut parts: Vec<String> = (0..rules.words)
        .map(|_| {
            let w = WORDS[rng.random_range(0..WORDS.len())];
            if rules.capitalise {
                let mut c = w.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            } else {
                w.to_string()
            }
        })
        .collect();
    if rules.add_number {
        parts.push(rng.random_range(0..100).to_string());
    }
    Ok(parts.join(&rules.separator))
}

/// How many bits of entropy a passphrase of `words` words carries, given
/// the list it is drawn from.
///
/// Reported in the UI because a passphrase from a short list looks far
/// stronger than it is, and the only honest thing to do is say so.
pub fn passphrase_entropy(words: usize) -> f64 {
    words as f64 * (WORDS.len() as f64).log2()
}

/// Which generator the page is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Uuid,
    Number,
    Password,
    Passphrase,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(default)]
pub struct State {
    pub mode: Mode,
    pub uuid_kind: UuidSetting,
    pub count: usize,
    pub low: i64,
    pub high: i64,
    /// Empty means "use fresh randomness"; a number makes the run repeat.
    pub seed: String,
    pub pw_length: usize,
    pub pw_lower: bool,
    pub pw_upper: bool,
    pub pw_digits: bool,
    pub pw_symbols: bool,
    pub pw_avoid_ambiguous: bool,
    pub phrase_words: usize,
    pub phrase_separator: String,
    pub phrase_capitalise: bool,
    pub phrase_number: bool,
    /// The last thing generated, kept so the tab reopens showing it.
    pub output: String,
}

impl Default for State {
    fn default() -> Self {
        let pw = PasswordRules::default();
        let ph = PassphraseRules::default();
        Self {
            mode: Mode::default(),
            uuid_kind: UuidSetting::default(),
            count: 5,
            low: 1,
            high: 100,
            seed: String::new(),
            pw_length: pw.length,
            pw_lower: pw.lower,
            pw_upper: pw.upper,
            pw_digits: pw.digits,
            pw_symbols: pw.symbols,
            pw_avoid_ambiguous: pw.avoid_ambiguous,
            phrase_words: ph.words,
            phrase_separator: ph.separator,
            phrase_capitalise: ph.capitalise,
            phrase_number: ph.add_number,
            output: String::new(),
        }
    }
}

/// The stored form of [`UuidKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UuidSetting {
    #[default]
    V4,
    V7,
    Nil,
}

impl From<UuidSetting> for UuidKind {
    fn from(v: UuidSetting) -> Self {
        match v {
            UuidSetting::V4 => UuidKind::V4,
            UuidSetting::V7 => UuidKind::V7,
            UuidSetting::Nil => UuidKind::Nil,
        }
    }
}

impl State {
    fn password_rules(&self) -> PasswordRules {
        PasswordRules {
            length: self.pw_length,
            lower: self.pw_lower,
            upper: self.pw_upper,
            digits: self.pw_digits,
            symbols: self.pw_symbols,
            avoid_ambiguous: self.pw_avoid_ambiguous,
        }
    }

    fn passphrase_rules(&self) -> PassphraseRules {
        PassphraseRules {
            words: self.phrase_words,
            separator: self.phrase_separator.clone(),
            capitalise: self.phrase_capitalise,
            add_number: self.phrase_number,
        }
    }

    fn seed_value(&self) -> Option<u64> {
        self.seed.trim().parse().ok()
    }
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &hs_core::Services, theme: &hs_ui::theme::Theme) {
    use hs_ui::form;
    form::page_title(
        ui,
        theme,
        &s.t("dt-generate"),
        Some(&s.t("dt-generate-sub")),
    );
    ui.horizontal(|ui| {
        ui.selectable_value(&mut st.mode, Mode::Uuid, s.t("dt-gen-uuid"));
        ui.selectable_value(&mut st.mode, Mode::Number, s.t("dt-gen-number"));
        ui.selectable_value(&mut st.mode, Mode::Password, s.t("dt-gen-password"));
        ui.selectable_value(&mut st.mode, Mode::Passphrase, s.t("dt-gen-passphrase"));
    });
    ui.add_space(8.0);
    let mut note: Option<String> = None;
    form::section(ui, theme, &s.t("dt-gen-options"), |ui| {
        match st.mode {
            Mode::Uuid => {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut st.uuid_kind, UuidSetting::V4, "v4");
                    ui.selectable_value(&mut st.uuid_kind, UuidSetting::V7, "v7");
                    ui.selectable_value(&mut st.uuid_kind, UuidSetting::Nil, s.t("dt-gen-nil"));
                });
                form::hint(ui, theme, &s.t("dt-gen-uuid-hint"));
            }
            Mode::Number => {
                ui.horizontal(|ui| {
                    ui.label(s.t("dt-gen-from"));
                    ui.add(egui::DragValue::new(&mut st.low));
                    ui.label(s.t("dt-gen-to"));
                    ui.add(egui::DragValue::new(&mut st.high));
                });
                form::hint(ui, theme, &s.t("dt-gen-seed-hint"));
            }
            Mode::Password => {
                ui.horizontal(|ui| {
                    ui.label(s.t("dt-gen-length"));
                    ui.add(egui::DragValue::new(&mut st.pw_length).range(1..=512));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut st.pw_lower, "a-z");
                    ui.checkbox(&mut st.pw_upper, "A-Z");
                    ui.checkbox(&mut st.pw_digits, "0-9");
                    ui.checkbox(&mut st.pw_symbols, s.t("dt-gen-symbols"));
                    ui.checkbox(&mut st.pw_avoid_ambiguous, s.t("dt-gen-no-ambiguous"));
                });
                note = Some(s.i18n.t_args(
                    "dt-gen-entropy",
                    &[(
                        "bits",
                        &format!("{:.0}", password_entropy(&st.password_rules())),
                    )],
                ));
            }
            Mode::Passphrase => {
                ui.horizontal(|ui| {
                    ui.label(s.t("dt-gen-words"));
                    ui.add(egui::DragValue::new(&mut st.phrase_words).range(1..=64));
                    ui.label(s.t("dt-gen-separator"));
                    ui.add(
                        egui::TextEdit::singleline(&mut st.phrase_separator).desired_width(60.0),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut st.phrase_capitalise, s.t("dt-gen-capitalise"));
                    ui.checkbox(&mut st.phrase_number, s.t("dt-gen-add-number"));
                });
                note = Some(s.i18n.t_args(
                    "dt-gen-phrase-entropy",
                    &[
                        (
                            "bits",
                            &format!("{:.0}", passphrase_entropy(st.phrase_words)),
                        ),
                        ("words", &WORDS.len().to_string()),
                    ],
                ));
            }
        }
        if st.mode != Mode::Password && st.mode != Mode::Passphrase {
            ui.horizontal(|ui| {
                ui.label(s.t("dt-gen-count"));
                ui.add(egui::DragValue::new(&mut st.count).range(1..=1000));
            });
        }
        ui.horizontal(|ui| {
            ui.label(s.t("dt-gen-seed"));
            ui.add(
                egui::TextEdit::singleline(&mut st.seed)
                    .hint_text(s.t("dt-gen-seed-empty"))
                    .desired_width(140.0),
            );
        });
        if let Some(n) = &note {
            form::hint(ui, theme, n);
        }
    });
    ui.add_space(8.0);
    let mut error: Option<String> = None;
    if form::primary_button(ui, theme, &s.t("dt-gen-go")).clicked() {
        let seed = st.seed_value();
        match st.mode {
            Mode::Uuid => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let mut rng = seeded(seed);
                st.output = (0..st.count)
                    .map(|_| uuid(st.uuid_kind.into(), &mut rng, now))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            Mode::Number => {
                st.output = numbers(st.low, st.high, st.count, seed)
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            Mode::Password => match password(&st.password_rules(), seed) {
                Ok(p) => st.output = p,
                Err(e) => error = Some(e),
            },
            Mode::Passphrase => match passphrase(&st.passphrase_rules(), seed) {
                Ok(p) => st.output = p,
                Err(e) => error = Some(e),
            },
        }
    }
    if let Some(e) = error {
        ui.add_space(6.0);
        ui.colored_label(theme.error, e);
    }
    if !st.output.is_empty() {
        ui.add_space(8.0);
        form::section(ui, theme, &s.t("dt-result"), |ui| {
            crate::util::output(ui, s, theme, &s.t("dt-gen-result"), &st.output, 12);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(42)
    }

    #[test]
    fn a_uuid_has_the_shape_and_the_version_it_claims() {
        let mut r = rng();
        for (kind, version) in [(UuidKind::V4, '4'), (UuidKind::V7, '7')] {
            let u = uuid(kind, &mut r, 1_700_000_000_000);
            assert_eq!(u.len(), 36, "{u}");
            let parts: Vec<&str> = u.split('-').collect();
            assert_eq!(
                parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
                [8, 4, 4, 4, 12]
            );
            assert!(u.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
            assert_eq!(parts[2].chars().next().unwrap(), version, "version in {u}");
            // The variant nibble is 8, 9, a or b.
            assert!("89ab".contains(parts[3].chars().next().unwrap()), "{u}");
        }
        assert_eq!(
            uuid(UuidKind::Nil, &mut r, 0),
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn v7_uuids_sort_in_the_order_they_were_made() {
        let mut r = rng();
        let early = uuid(UuidKind::V7, &mut r, 1_700_000_000_000);
        let later = uuid(UuidKind::V7, &mut r, 1_700_000_900_000);
        assert!(early < later, "{early} should sort before {later}");
    }

    #[test]
    fn the_same_seed_gives_the_same_numbers() {
        let a = numbers(1, 6, 10, Some(7));
        let b = numbers(1, 6, 10, Some(7));
        assert_eq!(a, b, "a seed is a promise of repeatability");
        assert_eq!(a.len(), 10);
        assert!(a.iter().all(|n| (1..=6).contains(n)));
        // Bounds the wrong way round are swapped, not refused.
        assert!(
            numbers(6, 1, 5, Some(7))
                .iter()
                .all(|n| (1..=6).contains(n))
        );
        // A single-value range is legal and gives that value.
        assert_eq!(numbers(4, 4, 3, Some(1)), [4, 4, 4]);
    }

    #[test]
    fn a_password_holds_one_of_every_kind_it_was_asked_for() {
        let rules = PasswordRules {
            length: 12,
            ..Default::default()
        };
        for seed in 0..50 {
            let p = password(&rules, Some(seed)).unwrap();
            assert_eq!(p.chars().count(), 12);
            assert!(p.chars().any(|c| LOWER.contains(c)), "no lower in {p}");
            assert!(p.chars().any(|c| UPPER.contains(c)), "no upper in {p}");
            assert!(p.chars().any(|c| DIGITS.contains(c)), "no digit in {p}");
            assert!(p.chars().any(|c| SYMBOLS.contains(c)), "no symbol in {p}");
        }
    }

    #[test]
    fn a_password_uses_only_the_kinds_it_was_asked_for() {
        let rules = PasswordRules {
            length: 30,
            upper: false,
            symbols: false,
            ..Default::default()
        };
        let p = password(&rules, Some(3)).unwrap();
        assert!(
            p.chars().all(|c| LOWER.contains(c) || DIGITS.contains(c)),
            "{p}"
        );
    }

    #[test]
    fn ambiguous_characters_can_be_left_out() {
        let rules = PasswordRules {
            length: 60,
            avoid_ambiguous: true,
            ..Default::default()
        };
        let p = password(&rules, Some(11)).unwrap();
        assert!(
            p.chars().all(|c| !AMBIGUOUS.contains(c)),
            "{p} still has an ambiguous character"
        );
    }

    #[test]
    fn impossible_password_rules_say_why() {
        let none = PasswordRules {
            lower: false,
            upper: false,
            digits: false,
            symbols: false,
            ..Default::default()
        };
        assert!(password(&none, None).is_err());
        let too_short = PasswordRules {
            length: 2,
            ..Default::default()
        };
        let e = password(&too_short, None).unwrap_err();
        assert!(e.contains("at least 4"), "{e}");
    }

    #[test]
    fn a_passphrase_joins_the_words_it_was_asked_for() {
        let rules = PassphraseRules {
            words: 4,
            separator: "-".into(),
            ..Default::default()
        };
        let p = passphrase(&rules, Some(5)).unwrap();
        assert_eq!(p.split('-').count(), 4, "{p}");
        assert!(p.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{p}");

        let fancy = PassphraseRules {
            words: 3,
            separator: ".".into(),
            capitalise: true,
            add_number: true,
        };
        let p = passphrase(&fancy, Some(5)).unwrap();
        let parts: Vec<&str> = p.split('.').collect();
        assert_eq!(parts.len(), 4, "three words and a number: {p}");
        assert!(parts[3].parse::<u32>().is_ok(), "{p}");
        for w in &parts[..3] {
            assert!(w.chars().next().unwrap().is_uppercase(), "{p}");
        }
        assert!(
            passphrase(
                &PassphraseRules {
                    words: 0,
                    ..Default::default()
                },
                None
            )
            .is_err()
        );
    }

    #[test]
    fn entropy_is_reported_honestly() {
        // 26+26+10+25 = 87 characters, 20 long: about 129 bits.
        let e = password_entropy(&PasswordRules::default());
        assert!((128.0..131.0).contains(&e), "{e}");
        // Dropping to lower case alone costs about 35 bits at this
        // length: a real loss, but not the halving one might guess, which
        // is why the number is shown rather than a word like "strong".
        let weak = password_entropy(&PasswordRules {
            upper: false,
            digits: false,
            symbols: false,
            ..Default::default()
        });
        assert!(e - weak > 30.0, "{weak} vs {e}");
        // A passphrase's entropy follows the list length, so the list must
        // be big enough to be worth offering.
        assert!(
            WORDS.len() >= 256,
            "the word list is too short to be useful"
        );
        // About 8.3 bits a word with this list, so the default of six is
        // worth roughly 50 — well short of a 7776-word diceware list's 77.
        // The UI shows the number for exactly that reason.
        let per_word = (WORDS.len() as f64).log2();
        assert!((8.0..9.0).contains(&per_word), "{per_word} bits a word");
        assert!(passphrase_entropy(6) > 48.0);
    }

    #[test]
    fn the_word_list_has_no_duplicates() {
        let mut seen: Vec<&str> = WORDS.to_vec();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "a repeated word costs entropy quietly");
    }
}
