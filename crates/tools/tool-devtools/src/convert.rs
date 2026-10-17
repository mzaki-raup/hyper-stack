//! Unit conversion: time, epochs, number bases, data sizes and durations.
//!
//! Every conversion is a plain function over its inputs, so the arithmetic
//! can be tested without a UI and the rounding rules are in one place
//! rather than scattered through widget code.
//!
//! Each kind of conversion keeps its **own** result, the way the generators
//! do: switching from Time to Numbers must not show a number produced by
//! the other pane, which would be worse than showing nothing.

use chrono::{DateTime, TimeZone, Utc};

// ---- time ---------------------------------------------------------------

/// A unit of time. Months and years are deliberately absent: neither has a
/// fixed length, so converting "1 month to hours" would have to invent an
/// answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeUnit {
    Nanoseconds,
    Microseconds,
    Milliseconds,
    #[default]
    Seconds,
    Minutes,
    Hours,
    Days,
    Weeks,
}

impl TimeUnit {
    pub const ALL: [TimeUnit; 8] = [
        TimeUnit::Nanoseconds,
        TimeUnit::Microseconds,
        TimeUnit::Milliseconds,
        TimeUnit::Seconds,
        TimeUnit::Minutes,
        TimeUnit::Hours,
        TimeUnit::Days,
        TimeUnit::Weeks,
    ];

    /// The Fluent key for this unit's name.
    pub fn key(self) -> &'static str {
        match self {
            TimeUnit::Nanoseconds => "dt-unit-ns",
            TimeUnit::Microseconds => "dt-unit-us",
            TimeUnit::Milliseconds => "dt-unit-ms",
            TimeUnit::Seconds => "dt-unit-s",
            TimeUnit::Minutes => "dt-unit-min",
            TimeUnit::Hours => "dt-unit-h",
            TimeUnit::Days => "dt-unit-d",
            TimeUnit::Weeks => "dt-unit-w",
        }
    }

    /// How many seconds one of these is.
    fn seconds(self) -> f64 {
        match self {
            TimeUnit::Nanoseconds => 1e-9,
            TimeUnit::Microseconds => 1e-6,
            TimeUnit::Milliseconds => 1e-3,
            TimeUnit::Seconds => 1.0,
            TimeUnit::Minutes => 60.0,
            TimeUnit::Hours => 3_600.0,
            TimeUnit::Days => 86_400.0,
            TimeUnit::Weeks => 604_800.0,
        }
    }
}

/// `value` of `from`, expressed in `to`.
pub fn time(value: f64, from: TimeUnit, to: TimeUnit) -> f64 {
    value * from.seconds() / to.seconds()
}

// ---- data size ----------------------------------------------------------

/// A unit of data. Both families are here on purpose: a disk is sold in
/// powers of ten and reported by the operating system in powers of two,
/// and being able to see the same number both ways is the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataUnit {
    Bits,
    #[default]
    Bytes,
    Kilobytes,
    Megabytes,
    Gigabytes,
    Terabytes,
    Kibibytes,
    Mebibytes,
    Gibibytes,
    Tebibytes,
}

impl DataUnit {
    pub const ALL: [DataUnit; 10] = [
        DataUnit::Bits,
        DataUnit::Bytes,
        DataUnit::Kilobytes,
        DataUnit::Megabytes,
        DataUnit::Gigabytes,
        DataUnit::Terabytes,
        DataUnit::Kibibytes,
        DataUnit::Mebibytes,
        DataUnit::Gibibytes,
        DataUnit::Tebibytes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DataUnit::Bits => "bit",
            DataUnit::Bytes => "B",
            DataUnit::Kilobytes => "kB",
            DataUnit::Megabytes => "MB",
            DataUnit::Gigabytes => "GB",
            DataUnit::Terabytes => "TB",
            DataUnit::Kibibytes => "KiB",
            DataUnit::Mebibytes => "MiB",
            DataUnit::Gibibytes => "GiB",
            DataUnit::Tebibytes => "TiB",
        }
    }

    fn bytes(self) -> f64 {
        match self {
            DataUnit::Bits => 0.125,
            DataUnit::Bytes => 1.0,
            DataUnit::Kilobytes => 1e3,
            DataUnit::Megabytes => 1e6,
            DataUnit::Gigabytes => 1e9,
            DataUnit::Terabytes => 1e12,
            DataUnit::Kibibytes => 1024.0,
            DataUnit::Mebibytes => 1024f64.powi(2),
            DataUnit::Gibibytes => 1024f64.powi(3),
            DataUnit::Tebibytes => 1024f64.powi(4),
        }
    }
}

pub fn data(value: f64, from: DataUnit, to: DataUnit) -> f64 {
    value * from.bytes() / to.bytes()
}

// ---- bandwidth ----------------------------------------------------------

/// A rate of data; every variant is per second, which the type says
/// rather than each name repeating it. Networking counts in bits and storage in bytes, which
/// is the whole reason a 100 Mb/s line copies a file at about 12 MB/s;
/// both families are here so that sum can be done without arithmetic in
/// the head. Owner's requirement, 2026-10-17.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RateUnit {
    Bit,
    Kilobit,
    #[default]
    Megabit,
    Gigabit,
    Byte,
    Kilobyte,
    Megabyte,
    Gigabyte,
    Mebibyte,
    Gibibyte,
}

impl RateUnit {
    pub const ALL: [RateUnit; 10] = [
        RateUnit::Bit,
        RateUnit::Kilobit,
        RateUnit::Megabit,
        RateUnit::Gigabit,
        RateUnit::Byte,
        RateUnit::Kilobyte,
        RateUnit::Megabyte,
        RateUnit::Gigabyte,
        RateUnit::Mebibyte,
        RateUnit::Gibibyte,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RateUnit::Bit => "bit/s",
            RateUnit::Kilobit => "kbit/s",
            RateUnit::Megabit => "Mbit/s",
            RateUnit::Gigabit => "Gbit/s",
            RateUnit::Byte => "B/s",
            RateUnit::Kilobyte => "kB/s",
            RateUnit::Megabyte => "MB/s",
            RateUnit::Gigabyte => "GB/s",
            RateUnit::Mebibyte => "MiB/s",
            RateUnit::Gibibyte => "GiB/s",
        }
    }

    /// How many bits per second one of these is.
    fn bits(self) -> f64 {
        match self {
            RateUnit::Bit => 1.0,
            RateUnit::Kilobit => 1e3,
            RateUnit::Megabit => 1e6,
            RateUnit::Gigabit => 1e9,
            RateUnit::Byte => 8.0,
            RateUnit::Kilobyte => 8e3,
            RateUnit::Megabyte => 8e6,
            RateUnit::Gigabyte => 8e9,
            RateUnit::Mebibyte => 8.0 * 1024f64.powi(2),
            RateUnit::Gibibyte => 8.0 * 1024f64.powi(3),
        }
    }
}

pub fn rate(value: f64, from: RateUnit, to: RateUnit) -> f64 {
    value * from.bits() / to.bits()
}

/// How long `size` bytes take at `value` of `unit`, in seconds. The other
/// question people bring to a bandwidth converter.
pub fn transfer_seconds(bytes: f64, value: f64, unit: RateUnit) -> Option<f64> {
    let bits_per_second = value * unit.bits();
    if bits_per_second <= 0.0 {
        return None;
    }
    Some(bytes * 8.0 / bits_per_second)
}

// ---- number bases -------------------------------------------------------

/// One number written every way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bases {
    pub decimal: String,
    pub binary: String,
    pub octal: String,
    pub hex: String,
    /// The character, when the value is a printable code point. A developer
    /// reading a hex dump wants to know that 0x41 is `A`.
    pub character: Option<String>,
}

/// Read a number written in `base`, allowing the usual prefixes, grouping
/// underscores and a leading sign.
pub fn parse_base(text: &str, base: u32) -> Option<i128> {
    let t = text.trim().replace(['_', ' '], "");
    if t.is_empty() {
        return None;
    }
    let (negative, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(&t)),
    };
    // A prefix is accepted only when it agrees with the base being read, so
    // "0x10" in base 10 is a mistake rather than silently 16.
    let digits = match base {
        2 => rest.strip_prefix("0b").or_else(|| rest.strip_prefix("0B")),
        8 => rest.strip_prefix("0o").or_else(|| rest.strip_prefix("0O")),
        16 => rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")),
        _ => None,
    }
    .unwrap_or(rest);
    if digits.is_empty() {
        return None;
    }
    let v = i128::from_str_radix(digits, base).ok()?;
    Some(if negative { -v } else { v })
}

/// Every representation of a value.
pub fn bases(v: i128) -> Bases {
    let sign = if v < 0 { "-" } else { "" };
    let m = v.unsigned_abs();
    Bases {
        decimal: v.to_string(),
        binary: format!("{sign}{m:b}"),
        octal: format!("{sign}{m:o}"),
        hex: format!("{sign}{m:X}"),
        character: u32::try_from(v)
            .ok()
            .and_then(char::from_u32)
            .filter(|c| !c.is_control())
            .map(|c| c.to_string()),
    }
}

// ---- durations ----------------------------------------------------------

/// Seconds as `1d 2h 3m 4s`, which is how a timeout or an uptime is read.
/// Zero parts are left out, except for zero itself.
pub fn humanise(total: i64) -> String {
    if total == 0 {
        return "0s".into();
    }
    let sign = if total < 0 { "-" } else { "" };
    let mut rest = total.unsigned_abs();
    let mut parts = Vec::new();
    for (size, suffix) in [
        (604_800u64, "w"),
        (86_400, "d"),
        (3_600, "h"),
        (60, "m"),
        (1, "s"),
    ] {
        let n = rest / size;
        if n > 0 {
            parts.push(format!("{n}{suffix}"));
            rest -= n * size;
        }
    }
    format!("{sign}{}", parts.join(" "))
}

/// Read `1d2h3m4s`, `90m`, `1h 30m` or a plain number of seconds.
pub fn parse_duration(text: &str) -> Option<i64> {
    let t = text.trim().to_lowercase().replace(' ', "");
    if t.is_empty() {
        return None;
    }
    let (negative, body) = match t.strip_prefix('-') {
        Some(r) => (true, r.to_string()),
        None => (false, t),
    };
    if let Ok(plain) = body.parse::<i64>() {
        return Some(if negative { -plain } else { plain });
    }
    let mut total: i64 = 0;
    let mut digits = String::new();
    let mut saw_any = false;
    for c in body.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let n: i64 = digits.parse().ok()?;
        digits.clear();
        let size = match c {
            'w' => 604_800,
            'd' => 86_400,
            'h' => 3_600,
            'm' => 60,
            's' => 1,
            _ => return None,
        };
        total = total.checked_add(n.checked_mul(size)?)?;
        saw_any = true;
    }
    // Trailing digits with no unit are not a duration: "1h30" is a typo,
    // and guessing at minutes would be worse than refusing.
    if !digits.is_empty() || !saw_any {
        return None;
    }
    Some(if negative { -total } else { total })
}

// ---- epochs -------------------------------------------------------------

/// How a timestamp is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EpochUnit {
    #[default]
    Seconds,
    Milliseconds,
}

/// An epoch timestamp as an ISO-8601 instant in UTC.
pub fn epoch_to_iso(value: i64, unit: EpochUnit) -> Option<String> {
    let dt: DateTime<Utc> = match unit {
        EpochUnit::Seconds => Utc.timestamp_opt(value, 0).single()?,
        EpochUnit::Milliseconds => Utc.timestamp_millis_opt(value).single()?,
    };
    Some(dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// An ISO-8601 instant as an epoch timestamp. A date with no time is read
/// as midnight UTC, which is what a date alone means here.
pub fn iso_to_epoch(text: &str, unit: EpochUnit) -> Option<i64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let dt = DateTime::parse_from_rfc3339(t)
        .map(|d| d.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|n| n.and_utc())
        })
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|n| n.and_utc())
        })?;
    Some(match unit {
        EpochUnit::Seconds => dt.timestamp(),
        EpochUnit::Milliseconds => dt.timestamp_millis(),
    })
}

/// Trim a converted number to something a person reads: no exponent for
/// ordinary sizes, no trailing zeros, and full precision kept for the very
/// large and very small rather than rounded away.
pub fn format_number(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    if v == 0.0 {
        return "0".into();
    }
    let magnitude = v.abs();
    if !(1e-6..1e15).contains(&magnitude) {
        return format!("{v:e}");
    }
    // Six decimals is enough for every unit here; strip what it does not need.
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

// ---- the UI -------------------------------------------------------------

/// Which conversion is on screen. Each keeps its own inputs and its own
/// result, so switching panes never shows a figure the other one produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    Time,
    Epoch,
    Number,
    Data,
    Duration,
    Bandwidth,
}

impl Pane {
    const ALL: [Pane; 6] = [
        Pane::Time,
        Pane::Epoch,
        Pane::Number,
        Pane::Data,
        Pane::Duration,
        Pane::Bandwidth,
    ];

    fn label_key(self) -> &'static str {
        match self {
            Pane::Time => "dt-conv-time",
            Pane::Epoch => "dt-conv-epoch",
            Pane::Number => "dt-conv-number",
            Pane::Data => "dt-conv-data",
            Pane::Duration => "dt-conv-duration",
            Pane::Bandwidth => "dt-conv-bandwidth",
        }
    }
}

/// Everything the converter remembers. Every pane has its own input and
/// its own result: a result card shared between them would show a time in
/// the numbers pane, which the owner asked against for the generators and
/// is no better here.
pub struct State {
    pub pane: Pane,
    // Time
    pub time_input: String,
    pub time_from: TimeUnit,
    pub time_to: TimeUnit,
    // Epoch
    pub epoch_input: String,
    pub epoch_unit: EpochUnit,
    pub iso_input: String,
    // Numbers
    pub number_input: String,
    pub number_base: u32,
    // Data
    pub data_input: String,
    pub data_from: DataUnit,
    pub data_to: DataUnit,
    // Durations
    pub duration_input: String,
    // Bandwidth
    pub rate_input: String,
    pub rate_from: RateUnit,
    pub rate_to: RateUnit,
    /// A size to time at that rate, in megabytes.
    pub transfer_mb: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            pane: Pane::default(),
            time_input: "1".into(),
            time_from: TimeUnit::Hours,
            time_to: TimeUnit::Seconds,
            epoch_input: String::new(),
            epoch_unit: EpochUnit::default(),
            iso_input: String::new(),
            number_input: String::new(),
            number_base: 10,
            data_input: "1".into(),
            data_from: DataUnit::Gigabytes,
            data_to: DataUnit::Gibibytes,
            duration_input: String::new(),
            rate_input: "100".into(),
            rate_from: RateUnit::Megabit,
            rate_to: RateUnit::Megabyte,
            transfer_mb: String::new(),
        }
    }
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &hs_core::Services, theme: &hs_ui::theme::Theme) {
    use hs_ui::form;
    form::page_title(ui, theme, &s.t("dt-convert"), Some(&s.t("dt-convert-sub")));
    ui.horizontal_wrapped(|ui| {
        for p in Pane::ALL {
            ui.selectable_value(&mut st.pane, p, s.t(p.label_key()));
        }
    });
    ui.add_space(8.0);
    match st.pane {
        Pane::Time => time_pane(st, ui, s, theme),
        Pane::Epoch => epoch_pane(st, ui, s, theme),
        Pane::Number => number_pane(st, ui, s, theme),
        Pane::Data => data_pane(st, ui, s, theme),
        Pane::Duration => duration_pane(st, ui, s, theme),
        Pane::Bandwidth => bandwidth_pane(st, ui, s, theme),
    }
}

/// A unit picker. Its own function so the two unit panes read the same.
fn unit_combo<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut T,
    all: &[T],
    label: impl Fn(T) -> String,
) {
    egui::ComboBox::from_id_salt(id)
        .width(140.0)
        .selected_text(label(*value))
        .show_ui(ui, |ui| {
            for u in all {
                ui.selectable_value(value, *u, label(*u));
            }
        });
}

fn time_pane(
    st: &mut State,
    ui: &mut egui::Ui,
    s: &hs_core::Services,
    theme: &hs_ui::theme::Theme,
) {
    use hs_ui::form;
    form::section(ui, theme, &s.t("dt-conv-time"), |ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut st.time_input)
                    .hint_text(s.t("dt-conv-value"))
                    .desired_width(120.0),
            );
            unit_combo(ui, "dt-time-from", &mut st.time_from, &TimeUnit::ALL, |u| {
                s.t(u.key())
            });
            ui.label("\u{2192}");
            unit_combo(ui, "dt-time-to", &mut st.time_to, &TimeUnit::ALL, |u| {
                s.t(u.key())
            });
            if ui
                .small_button(hs_ui::icons::ph::ARROWS_LEFT_RIGHT)
                .on_hover_text(s.t("dt-conv-swap"))
                .clicked()
            {
                std::mem::swap(&mut st.time_from, &mut st.time_to);
            }
        });
    });
    let parsed = st.time_input.trim().parse::<f64>().ok();
    match parsed {
        Some(v) => {
            let out = format_number(time(v, st.time_from, st.time_to));
            crate::util::output(
                ui,
                s,
                theme,
                &s.t("dt-conv-result"),
                &format!("{out} {}", s.t(st.time_to.key())),
                1,
            );
            // Every unit at once: the answer that was not asked for is
            // often the one wanted next.
            form::section(ui, theme, &s.t("dt-conv-all-units"), |ui| {
                for u in TimeUnit::ALL {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(s.t(u.key()))
                                .color(theme.text_muted)
                                .monospace(),
                        );
                        ui.label(
                            egui::RichText::new(format_number(time(v, st.time_from, u)))
                                .monospace(),
                        );
                    });
                }
            });
        }
        None if !st.time_input.trim().is_empty() => {
            crate::util::error(ui, theme, &s.t("dt-conv-not-a-number"))
        }
        None => {}
    }
}

fn data_pane(
    st: &mut State,
    ui: &mut egui::Ui,
    s: &hs_core::Services,
    theme: &hs_ui::theme::Theme,
) {
    use hs_ui::form;
    form::section(ui, theme, &s.t("dt-conv-data"), |ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut st.data_input)
                    .hint_text(s.t("dt-conv-value"))
                    .desired_width(120.0),
            );
            unit_combo(ui, "dt-data-from", &mut st.data_from, &DataUnit::ALL, |u| {
                u.label().to_string()
            });
            ui.label("\u{2192}");
            unit_combo(ui, "dt-data-to", &mut st.data_to, &DataUnit::ALL, |u| {
                u.label().to_string()
            });
            if ui
                .small_button(hs_ui::icons::ph::ARROWS_LEFT_RIGHT)
                .on_hover_text(s.t("dt-conv-swap"))
                .clicked()
            {
                std::mem::swap(&mut st.data_from, &mut st.data_to);
            }
        });
        form::hint(ui, theme, &s.t("dt-conv-data-hint"));
    });
    let parsed = st.data_input.trim().parse::<f64>().ok();
    match parsed {
        Some(v) => {
            let out = format_number(data(v, st.data_from, st.data_to));
            crate::util::output(
                ui,
                s,
                theme,
                &s.t("dt-conv-result"),
                &format!("{out} {}", st.data_to.label()),
                1,
            );
            form::section(ui, theme, &s.t("dt-conv-all-units"), |ui| {
                for u in DataUnit::ALL {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(u.label())
                                .color(theme.text_muted)
                                .monospace(),
                        );
                        ui.label(
                            egui::RichText::new(format_number(data(v, st.data_from, u)))
                                .monospace(),
                        );
                    });
                }
            });
        }
        None if !st.data_input.trim().is_empty() => {
            crate::util::error(ui, theme, &s.t("dt-conv-not-a-number"))
        }
        None => {}
    }
}

fn number_pane(
    st: &mut State,
    ui: &mut egui::Ui,
    s: &hs_core::Services,
    theme: &hs_ui::theme::Theme,
) {
    use hs_ui::form;
    form::section(ui, theme, &s.t("dt-conv-number"), |ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut st.number_input)
                    .hint_text(s.t("dt-conv-number-hint"))
                    .desired_width(220.0),
            );
            for (base, label) in [(10u32, "DEC"), (2, "BIN"), (8, "OCT"), (16, "HEX")] {
                ui.selectable_value(&mut st.number_base, base, label);
            }
        });
        form::hint(ui, theme, &s.t("dt-conv-number-note"));
    });
    if st.number_input.trim().is_empty() {
        return;
    }
    let Some(v) = parse_base(&st.number_input, st.number_base) else {
        crate::util::error(ui, theme, &s.t("dt-conv-bad-number"));
        return;
    };
    let b = bases(v);
    for (label, value) in [
        ("DEC", &b.decimal),
        ("BIN", &b.binary),
        ("OCT", &b.octal),
        ("HEX", &b.hex),
    ] {
        crate::util::output(ui, s, theme, label, value, 1);
    }
    if let Some(c) = &b.character {
        crate::util::output(ui, s, theme, &s.t("dt-conv-character"), c, 1);
    }
}

fn epoch_pane(
    st: &mut State,
    ui: &mut egui::Ui,
    s: &hs_core::Services,
    theme: &hs_ui::theme::Theme,
) {
    use hs_ui::form;
    form::section(ui, theme, &s.t("dt-conv-epoch"), |ui| {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut st.epoch_unit, EpochUnit::Seconds, s.t("dt-unit-s"));
            ui.selectable_value(
                &mut st.epoch_unit,
                EpochUnit::Milliseconds,
                s.t("dt-unit-ms"),
            );
            if ui.small_button(s.t("dt-conv-now")).clicked() {
                let now = chrono::Utc::now();
                st.epoch_input = match st.epoch_unit {
                    EpochUnit::Seconds => now.timestamp().to_string(),
                    EpochUnit::Milliseconds => now.timestamp_millis().to_string(),
                };
                st.iso_input.clear();
            }
        });
        ui.add_space(4.0);
        ui.label(s.t("dt-conv-epoch-value"));
        ui.add(
            egui::TextEdit::singleline(&mut st.epoch_input)
                .hint_text("1760659200")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        ui.label(s.t("dt-conv-iso-value"));
        ui.add(
            egui::TextEdit::singleline(&mut st.iso_input)
                .hint_text("2026-10-17T00:00:00Z")
                .desired_width(f32::INFINITY),
        );
        form::hint(ui, theme, &s.t("dt-conv-epoch-hint"));
    });
    // Whichever side has text is the question; the other is the answer.
    if !st.epoch_input.trim().is_empty() {
        match st
            .epoch_input
            .trim()
            .parse::<i64>()
            .ok()
            .and_then(|v| epoch_to_iso(v, st.epoch_unit).map(|iso| (v, iso)))
        {
            Some((v, iso)) => {
                crate::util::output(ui, s, theme, &s.t("dt-conv-iso-value"), &iso, 1);
                let other = match st.epoch_unit {
                    EpochUnit::Seconds => format!("{} ms", v.saturating_mul(1000)),
                    EpochUnit::Milliseconds => format!("{} s", v / 1000),
                };
                crate::util::output(ui, s, theme, &s.t("dt-conv-other-unit"), &other, 1);
            }
            None => crate::util::error(ui, theme, &s.t("dt-conv-bad-epoch")),
        }
    } else if !st.iso_input.trim().is_empty() {
        match iso_to_epoch(&st.iso_input, st.epoch_unit) {
            Some(v) => {
                crate::util::output(ui, s, theme, &s.t("dt-conv-epoch-value"), &v.to_string(), 1)
            }
            None => crate::util::error(ui, theme, &s.t("dt-conv-bad-iso")),
        }
    }
}

fn bandwidth_pane(
    st: &mut State,
    ui: &mut egui::Ui,
    s: &hs_core::Services,
    theme: &hs_ui::theme::Theme,
) {
    use hs_ui::form;
    form::section(ui, theme, &s.t("dt-conv-bandwidth"), |ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut st.rate_input)
                    .hint_text(s.t("dt-conv-value"))
                    .desired_width(120.0),
            );
            unit_combo(ui, "dt-rate-from", &mut st.rate_from, &RateUnit::ALL, |u| {
                u.label().to_string()
            });
            ui.label("→");
            unit_combo(ui, "dt-rate-to", &mut st.rate_to, &RateUnit::ALL, |u| {
                u.label().to_string()
            });
            if ui
                .small_button(hs_ui::icons::ph::ARROWS_LEFT_RIGHT)
                .on_hover_text(s.t("dt-conv-swap"))
                .clicked()
            {
                std::mem::swap(&mut st.rate_from, &mut st.rate_to);
            }
        });
        form::hint(ui, theme, &s.t("dt-conv-bandwidth-hint"));
    });
    let Some(v) = st.rate_input.trim().parse::<f64>().ok() else {
        if !st.rate_input.trim().is_empty() {
            crate::util::error(ui, theme, &s.t("dt-conv-not-a-number"));
        }
        return;
    };
    let out = format_number(rate(v, st.rate_from, st.rate_to));
    crate::util::output(
        ui,
        s,
        theme,
        &s.t("dt-conv-result"),
        &format!("{out} {}", st.rate_to.label()),
        1,
    );
    form::section(ui, theme, &s.t("dt-conv-all-units"), |ui| {
        for u in RateUnit::ALL {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(u.label())
                        .color(theme.text_muted)
                        .monospace(),
                );
                ui.label(egui::RichText::new(format_number(rate(v, st.rate_from, u))).monospace());
            });
        }
    });
    // How long something takes at this rate: the other half of the
    // question people bring to a bandwidth converter.
    form::section(ui, theme, &s.t("dt-conv-transfer"), |ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut st.transfer_mb)
                    .hint_text(s.t("dt-conv-transfer-hint"))
                    .desired_width(120.0),
            );
            ui.label("MB");
        });
    });
    if st.transfer_mb.trim().is_empty() {
        return;
    }
    match st
        .transfer_mb
        .trim()
        .parse::<f64>()
        .ok()
        .and_then(|mb| transfer_seconds(mb * 1e6, v, st.rate_from))
    {
        Some(secs) => crate::util::output(
            ui,
            s,
            theme,
            &s.t("dt-conv-transfer-time"),
            &format!(
                "{} ({} s)",
                humanise(secs.round() as i64),
                format_number(secs)
            ),
            1,
        ),
        None => crate::util::error(ui, theme, &s.t("dt-conv-not-a-number")),
    }
}

fn duration_pane(
    st: &mut State,
    ui: &mut egui::Ui,
    s: &hs_core::Services,
    theme: &hs_ui::theme::Theme,
) {
    use hs_ui::form;
    form::section(ui, theme, &s.t("dt-conv-duration"), |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut st.duration_input)
                .hint_text(s.t("dt-conv-duration-hint"))
                .desired_width(f32::INFINITY),
        );
        form::hint(ui, theme, &s.t("dt-conv-duration-note"));
    });
    if st.duration_input.trim().is_empty() {
        return;
    }
    let Some(total) = parse_duration(&st.duration_input) else {
        crate::util::error(ui, theme, &s.t("dt-conv-bad-duration"));
        return;
    };
    crate::util::output(ui, s, theme, &s.t("dt-conv-readable"), &humanise(total), 1);
    for (key, unit) in [
        ("dt-unit-s", TimeUnit::Seconds),
        ("dt-unit-min", TimeUnit::Minutes),
        ("dt-unit-h", TimeUnit::Hours),
        ("dt-unit-d", TimeUnit::Days),
    ] {
        let v = time(total as f64, TimeUnit::Seconds, unit);
        crate::util::output(ui, s, theme, &s.t(key), &format_number(v), 1);
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;

    #[test]
    fn every_pane_starts_with_its_own_input() {
        let st = State::default();
        // Each pane owns its field: a value typed in one must not appear in
        // another, which is what a shared result card would do.
        assert_eq!(st.time_input, "1");
        assert_eq!(st.data_input, "1");
        assert!(st.number_input.is_empty());
        assert!(st.duration_input.is_empty());
        assert!(st.epoch_input.is_empty());
    }

    #[test]
    fn the_default_data_conversion_is_the_one_people_actually_ask() {
        // "Why is my 1 GB not 1 GiB?" is the question that brings someone
        // to a converter in the first place.
        let st = State::default();
        assert_eq!(st.data_from, DataUnit::Gigabytes);
        assert_eq!(st.data_to, DataUnit::Gibibytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_converts_both_ways_and_round_trips() {
        assert_eq!(time(1.0, TimeUnit::Days, TimeUnit::Hours), 24.0);
        assert_eq!(time(1.0, TimeUnit::Hours, TimeUnit::Seconds), 3600.0);
        assert_eq!(time(1.0, TimeUnit::Hours, TimeUnit::Milliseconds), 3.6e6);
        assert_eq!(time(90.0, TimeUnit::Minutes, TimeUnit::Hours), 1.5);
        let there = time(2.5, TimeUnit::Weeks, TimeUnit::Microseconds);
        let back = time(there, TimeUnit::Microseconds, TimeUnit::Weeks);
        assert!((back - 2.5).abs() < 1e-9, "{back}");
    }

    #[test]
    fn data_sizes_keep_the_two_families_apart() {
        assert_eq!(data(1.0, DataUnit::Kilobytes, DataUnit::Bytes), 1000.0);
        assert_eq!(data(1.0, DataUnit::Kibibytes, DataUnit::Bytes), 1024.0);
        assert_eq!(data(1.0, DataUnit::Bytes, DataUnit::Bits), 8.0);
        // The classic "why is my 1 TB disk 931 GiB?"
        let gib = data(1.0, DataUnit::Terabytes, DataUnit::Gibibytes);
        assert!((gib - 931.32).abs() < 0.01, "{gib}");
    }

    #[test]
    fn bandwidth_crosses_between_bits_and_bytes() {
        // The classic: a 100 Mbit/s line is about 12.5 MB/s.
        let mbs = rate(100.0, RateUnit::Megabit, RateUnit::Megabyte);
        assert!((mbs - 12.5).abs() < 1e-9, "{mbs}");
        assert_eq!(rate(1.0, RateUnit::Byte, RateUnit::Bit), 8.0);
        // And the two byte families stay apart, as in Data Size.
        let mib = rate(1.0, RateUnit::Megabyte, RateUnit::Mebibyte);
        assert!((mib - 0.953_674).abs() < 1e-5, "{mib}");
    }

    #[test]
    fn a_transfer_time_is_size_over_rate() {
        // 1 GB over 100 Mbit/s is about 80 seconds.
        let secs = transfer_seconds(1e9, 100.0, RateUnit::Megabit).unwrap();
        assert!((secs - 80.0).abs() < 1e-9, "{secs}");
        // A rate of zero has no answer rather than an infinite one.
        assert_eq!(transfer_seconds(1e9, 0.0, RateUnit::Megabit), None);
    }

    #[test]
    fn a_number_is_read_in_its_base_and_written_in_all_of_them() {
        assert_eq!(parse_base("255", 10), Some(255));
        assert_eq!(parse_base("0xFF", 16), Some(255));
        assert_eq!(parse_base("ff", 16), Some(255));
        assert_eq!(parse_base("1111_1111", 2), Some(255));
        assert_eq!(parse_base("-10", 10), Some(-10));
        let b = bases(255);
        assert_eq!(b.binary, "11111111");
        assert_eq!(b.octal, "377");
        assert_eq!(b.hex, "FF");
        assert_eq!(bases(65).character.as_deref(), Some("A"));
    }

    #[test]
    fn a_prefix_that_disagrees_with_the_base_is_refused() {
        // "0x10" read as decimal is a mistake, not sixteen.
        assert_eq!(parse_base("0x10", 10), None);
        assert_eq!(parse_base("2", 2), None, "2 is not a binary digit");
        assert_eq!(parse_base("", 10), None);
        assert_eq!(parse_base("   ", 16), None);
    }

    #[test]
    fn durations_are_written_and_read_back() {
        assert_eq!(humanise(0), "0s");
        assert_eq!(humanise(93_784), "1d 2h 3m 4s");
        assert_eq!(humanise(-90), "-1m 30s");
        assert_eq!(humanise(604_800), "1w");
        assert_eq!(parse_duration("1d2h3m4s"), Some(93_784));
        assert_eq!(parse_duration("1h 30m"), Some(5_400));
        assert_eq!(parse_duration("90"), Some(90));
        assert_eq!(parse_duration("-90s"), Some(-90));
        // Round trip through the text a person would read.
        let text = humanise(123_456);
        assert_eq!(parse_duration(&text), Some(123_456));
    }

    #[test]
    fn a_duration_with_a_dangling_number_is_refused() {
        assert_eq!(parse_duration("1h30"), None, "30 of what?");
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn epochs_convert_both_ways() {
        // 2026-10-17T00:00:00Z
        let secs =
            iso_to_epoch("2026-10-17", EpochUnit::Seconds).expect("a date alone is midnight");
        assert_eq!(
            epoch_to_iso(secs, EpochUnit::Seconds).as_deref(),
            Some("2026-10-17T00:00:00.000Z")
        );
        let ms = iso_to_epoch("2026-10-17T12:30:45Z", EpochUnit::Milliseconds).unwrap();
        assert_eq!(ms % 1000, 0);
        assert_eq!(
            ms / 1000,
            iso_to_epoch("2026-10-17T12:30:45Z", EpochUnit::Seconds).unwrap()
        );
        assert_eq!(iso_to_epoch("not a date", EpochUnit::Seconds), None);
    }

    #[test]
    fn numbers_are_formatted_for_reading_not_for_machines() {
        assert_eq!(format_number(1.0), "1");
        assert_eq!(format_number(1.5), "1.5");
        assert_eq!(format_number(3_600_000.0), "3600000");
        assert_eq!(format_number(0.0), "0");
        // Outside the readable range an exponent is clearer than 20 zeros.
        assert!(format_number(1e20).contains('e'));
    }
}
