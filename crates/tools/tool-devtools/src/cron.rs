//! Cron expressions: build one from a plain description of when a job
//! should run, and check one that was typed in.
//!
//! The three flavours differ in ways that are easy to get wrong by hand —
//! Spring and Quartz lead with a seconds field that Unix cron does not
//! have, and Quartz refuses to take both a day-of-month and a day-of-week,
//! wanting `?` in whichever one is not being used. Building all three from
//! the same description is the point: you can read off the one you need
//! without translating.

/// Which dialect to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Flavour {
    /// Five fields: minute hour day-of-month month day-of-week.
    #[default]
    Unix,
    /// Spring's `@Scheduled(cron = …)`: six fields, seconds first.
    Spring,
    /// Quartz: six or seven fields, seconds first, and `?` in exactly one
    /// of day-of-month or day-of-week.
    Quartz,
}

impl Flavour {
    pub const ALL: [Flavour; 3] = [Flavour::Unix, Flavour::Spring, Flavour::Quartz];

    pub fn label(self) -> &'static str {
        match self {
            Flavour::Unix => "Unix cron",
            Flavour::Spring => "Spring",
            Flavour::Quartz => "Quartz",
        }
    }

    /// How many fields an expression of this flavour has.
    fn fields(self) -> std::ops::RangeInclusive<usize> {
        match self {
            Flavour::Unix => 5..=5,
            Flavour::Spring => 6..=6,
            Flavour::Quartz => 6..=7,
        }
    }

    fn has_seconds(self) -> bool {
        !matches!(self, Flavour::Unix)
    }
}

/// How often a job runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Frequency {
    EveryNMinutes,
    EveryNHours,
    #[default]
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// The days of the week, Sunday first — the order every cron dialect
/// numbers them in.
pub const DAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
pub const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// When a job should run, as the page's fields describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    pub frequency: Frequency,
    /// For the every-N frequencies.
    pub interval: u32,
    pub minute: u32,
    pub hour: u32,
    /// Sunday first, matching [`DAY_NAMES`].
    pub days: [bool; 7],
    pub day_of_month: u32,
    /// 1-12.
    pub month: u32,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            frequency: Frequency::default(),
            interval: 5,
            minute: 0,
            hour: 9,
            days: [false, true, true, true, true, true, false],
            day_of_month: 1,
            month: 1,
        }
    }
}

/// The expression for this schedule in that dialect, or why it cannot be
/// written.
pub fn expression(s: &Schedule, flavour: Flavour) -> Result<String, String> {
    if s.minute > 59 {
        return Err("a minute is 0 to 59".into());
    }
    if s.hour > 23 {
        return Err("an hour is 0 to 23".into());
    }
    if !(1..=31).contains(&s.day_of_month) {
        return Err("a day of the month is 1 to 31".into());
    }
    if !(1..=12).contains(&s.month) {
        return Err("a month is 1 to 12".into());
    }
    let (minute, hour, dom, month, dow) = match s.frequency {
        Frequency::EveryNMinutes => {
            if !(1..=59).contains(&s.interval) {
                return Err("minutes between runs must be 1 to 59".into());
            }
            (
                format!("*/{}", s.interval),
                "*".into(),
                star(),
                star(),
                any_for(flavour),
            )
        }
        Frequency::EveryNHours => {
            if !(1..=23).contains(&s.interval) {
                return Err("hours between runs must be 1 to 23".into());
            }
            (
                s.minute.to_string(),
                format!("*/{}", s.interval),
                star(),
                star(),
                any_for(flavour),
            )
        }
        Frequency::Daily => (
            s.minute.to_string(),
            s.hour.to_string(),
            star(),
            star(),
            any_for(flavour),
        ),
        Frequency::Weekly => {
            let picked: Vec<String> = s
                .days
                .iter()
                .enumerate()
                .filter(|(_, on)| **on)
                .map(|(i, _)| i.to_string())
                .collect();
            if picked.is_empty() {
                return Err("choose at least one day of the week".into());
            }
            (
                s.minute.to_string(),
                s.hour.to_string(),
                // Quartz will not take a day-of-month and a day-of-week at
                // once; the one not in use must be `?`.
                if flavour == Flavour::Quartz {
                    "?".into()
                } else {
                    "*".into()
                },
                star(),
                picked.join(","),
            )
        }
        Frequency::Monthly => (
            s.minute.to_string(),
            s.hour.to_string(),
            s.day_of_month.to_string(),
            star(),
            any_for(flavour),
        ),
        Frequency::Yearly => (
            s.minute.to_string(),
            s.hour.to_string(),
            s.day_of_month.to_string(),
            s.month.to_string(),
            any_for(flavour),
        ),
    };
    let mut parts: Vec<String> = Vec::new();
    if flavour.has_seconds() {
        parts.push("0".into());
    }
    parts.push(minute);
    parts.push(hour);
    parts.push(dom);
    parts.push(month);
    parts.push(dow);
    Ok(parts.join(" "))
}

fn star() -> String {
    "*".into()
}

/// The day-of-week field when no particular day is meant: `?` for Quartz,
/// which refuses `*` in both day fields, and `*` for the rest.
fn any_for(flavour: Flavour) -> String {
    if flavour == Flavour::Quartz {
        "?".into()
    } else {
        "*".into()
    }
}

/// What the schedule means, in words, so the expression can be checked
/// against the intention rather than read character by character.
pub fn describe(s: &Schedule) -> String {
    let at = format!("{:02}:{:02}", s.hour, s.minute);
    match s.frequency {
        Frequency::EveryNMinutes => match s.interval {
            1 => "Every minute.".into(),
            n => format!("Every {n} minutes."),
        },
        Frequency::EveryNHours => match s.interval {
            1 => format!("Every hour, at {:02} minutes past.", s.minute),
            n => format!("Every {n} hours, at {:02} minutes past.", s.minute),
        },
        Frequency::Daily => format!("Every day at {at}."),
        Frequency::Weekly => {
            let names: Vec<&str> = s
                .days
                .iter()
                .enumerate()
                .filter(|(_, on)| **on)
                .map(|(i, _)| DAY_NAMES[i])
                .collect();
            if names.is_empty() {
                "No days chosen.".into()
            } else {
                format!("Every {} at {at}.", names.join(", "))
            }
        }
        Frequency::Monthly => format!("On day {} of every month at {at}.", s.day_of_month),
        Frequency::Yearly => format!(
            "On {} {} every year at {at}.",
            s.day_of_month,
            MONTH_NAMES[(s.month.clamp(1, 12) - 1) as usize]
        ),
    }
}

/// Whether a typed expression is shaped like that dialect.
///
/// Field counts and ranges only: this says an expression is plausible, not
/// that a particular scheduler will accept every name and modifier it
/// carries (`L`, `W`, `#` and friends are allowed through unchecked).
pub fn check(expr: &str, flavour: Flavour) -> Result<(), String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.is_empty() {
        return Err("nothing to check".into());
    }
    let want = flavour.fields();
    if !want.contains(&parts.len()) {
        let wanted = if want.start() == want.end() {
            format!("{}", want.start())
        } else {
            format!("{} or {}", want.start(), want.end())
        };
        return Err(format!(
            "{} takes {wanted} fields; this has {}",
            flavour.label(),
            parts.len()
        ));
    }
    let offset = usize::from(flavour.has_seconds());
    let ranges: [(&str, u32, u32); 5] = [
        ("minute", 0, 59),
        ("hour", 0, 23),
        ("day of the month", 1, 31),
        ("month", 1, 12),
        ("day of the week", 0, 7),
    ];
    if flavour.has_seconds() {
        check_field(parts[0], "second", 0, 59)?;
    }
    for (i, (name, lo, hi)) in ranges.iter().enumerate() {
        check_field(parts[offset + i], name, *lo, *hi)?;
    }
    if flavour == Flavour::Quartz {
        let dom = parts[offset + 2];
        let dow = parts[offset + 4];
        let q = |f: &str| f == "?";
        if q(dom) == q(dow) {
            return Err("Quartz wants ? in exactly one of day-of-month and day-of-week".into());
        }
    }
    Ok(())
}

/// What a typed expression means, in words.
///
/// The reverse of [`expression`]: it reads the fields back so an
/// expression someone else wrote can be checked against the intention.
/// Fields it cannot put into a sentence are named rather than guessed at —
/// "day of the week 2#1" is more use than a confident wrong reading.
pub fn explain(expr: &str, flavour: Flavour) -> Result<String, String> {
    check(expr, flavour)?;
    let parts: Vec<&str> = expr.split_whitespace().collect();
    let offset = usize::from(flavour.has_seconds());
    let (second, minute, hour) = (
        if flavour.has_seconds() {
            Some(parts[0])
        } else {
            None
        },
        parts[offset],
        parts[offset + 1],
    );
    let (dom, month, dow) = (parts[offset + 2], parts[offset + 3], parts[offset + 4]);

    let mut when = Vec::new();
    // The common shapes first, because they are what people write.
    if let Some(step) = step_of(minute)
        && hour == "*"
    {
        when.push(format!("every {step} minutes"));
    } else if let Some(step) = step_of(hour) {
        when.push(format!(
            "every {step} hours at {} minutes past",
            plain(minute).unwrap_or_else(|| minute.into())
        ));
    } else if let (Some(h), Some(m)) = (plain(hour), plain(minute)) {
        when.push(format!("at {h:0>2}:{m:0>2}"));
    } else if minute == "*" && hour == "*" {
        when.push("every minute".into());
    } else {
        when.push(format!("minute {minute}, hour {hour}"));
    }
    if let Some(s) = second
        && s != "0"
    {
        when.push(match step_of(s) {
            Some(step) => format!("every {step} seconds"),
            None => format!("second {s}"),
        });
    }

    let mut days = Vec::new();
    if dow != "*" && dow != "?" {
        days.push(format!("on {}", day_names(dow)));
    }
    if dom != "*" && dom != "?" {
        days.push(format!("on day {dom} of the month"));
    }
    if month != "*" && month != "?" {
        days.push(format!("in {}", month_names(month)));
    }
    if days.is_empty() {
        days.push("every day".into());
    }
    let mut out = format!("{}, {}.", when.join(", "), days.join(", "));
    if let Some(rest) = out.strip_prefix(char::is_whitespace) {
        out = rest.to_string();
    }
    // A sentence reads better starting with a capital.
    let mut c = out.chars();
    Ok(match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => out,
    })
}

/// The step of a `*/n` field.
fn step_of(field: &str) -> Option<u32> {
    let (base, step) = field.split_once('/')?;
    if base != "*" {
        return None;
    }
    step.parse().ok()
}

/// A field that is one plain number.
fn plain(field: &str) -> Option<String> {
    field.parse::<u32>().ok().map(|n| n.to_string())
}

/// Day-of-week numbers as names, leaving anything clever as it was.
fn day_names(field: &str) -> String {
    let named: Option<Vec<&str>> = field
        .split(',')
        .map(|p| {
            p.parse::<usize>()
                .ok()
                .and_then(|n| DAY_NAMES.get(n % 7).copied())
        })
        .collect();
    match named {
        Some(names) => names.join(", "),
        // Ranges, names and Quartz's `#` forms: say the field rather than
        // inventing a reading of it.
        None => format!("day of the week {field}"),
    }
}

fn month_names(field: &str) -> String {
    let named: Option<Vec<&str>> = field
        .split(',')
        .map(|p| {
            p.parse::<usize>()
                .ok()
                .filter(|n| (1..=12).contains(n))
                .and_then(|n| MONTH_NAMES.get(n - 1).copied())
        })
        .collect();
    match named {
        Some(names) => names.join(", "),
        None => format!("month {field}"),
    }
}

/// One field: `*`, `?`, a number, a list, a range or a step, with every
/// number inside the range for that field.
fn check_field(field: &str, name: &str, lo: u32, hi: u32) -> Result<(), String> {
    if field.is_empty() {
        return Err(format!("the {name} field is empty"));
    }
    for part in field.split(',') {
        let (value, step) = match part.split_once('/') {
            Some((v, s)) => (v, Some(s)),
            None => (part, None),
        };
        if let Some(s) = step
            && s.parse::<u32>().map(|n| n == 0).unwrap_or(true)
        {
            return Err(format!(
                "{part}: the step in the {name} field is not a number"
            ));
        }
        if value == "*" || value == "?" {
            continue;
        }
        // Names and modifiers (MON, JAN, L, W, 2#1) are passed through:
        // the dialects differ on which they take, and refusing them here
        // would be wrong more often than right. Only plain numbers, lists
        // and ranges are range-checked.
        if value.chars().any(|c| !c.is_ascii_digit() && c != '-') {
            continue;
        }
        for n in value.split('-') {
            let Ok(v) = n.parse::<u32>() else {
                return Err(format!("{part}: the {name} field is not a number"));
            };
            if v < lo || v > hi {
                return Err(format!("{part}: a {name} is {lo} to {hi}"));
            }
        }
    }
    Ok(())
}

/// The stored form of [`Frequency`], so a saved tab reopens as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FrequencySetting {
    EveryNMinutes,
    EveryNHours,
    #[default]
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl From<FrequencySetting> for Frequency {
    fn from(v: FrequencySetting) -> Self {
        match v {
            FrequencySetting::EveryNMinutes => Frequency::EveryNMinutes,
            FrequencySetting::EveryNHours => Frequency::EveryNHours,
            FrequencySetting::Daily => Frequency::Daily,
            FrequencySetting::Weekly => Frequency::Weekly,
            FrequencySetting::Monthly => Frequency::Monthly,
            FrequencySetting::Yearly => Frequency::Yearly,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(default)]
pub struct State {
    pub frequency: FrequencySetting,
    pub interval: u32,
    pub minute: u32,
    pub hour: u32,
    pub days: [bool; 7],
    pub day_of_month: u32,
    pub month: u32,
    /// An expression pasted in to be checked, which is the other half of
    /// the job: reading one somebody else wrote.
    pub check_input: String,
}

impl Default for State {
    fn default() -> Self {
        let s = Schedule::default();
        Self {
            frequency: FrequencySetting::default(),
            interval: s.interval,
            minute: s.minute,
            hour: s.hour,
            days: s.days,
            day_of_month: s.day_of_month,
            month: s.month,
            check_input: String::new(),
        }
    }
}

impl State {
    fn schedule(&self) -> Schedule {
        Schedule {
            frequency: self.frequency.into(),
            interval: self.interval,
            minute: self.minute,
            hour: self.hour,
            days: self.days,
            day_of_month: self.day_of_month,
            month: self.month,
        }
    }
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &hs_core::Services, theme: &hs_ui::theme::Theme) {
    use hs_ui::form;
    form::page_title(ui, theme, &s.t("dt-cron"), Some(&s.t("dt-cron-sub")));
    ui.add_space(4.0);
    form::section(ui, theme, &s.t("dt-cron-when"), |ui| {
        ui.horizontal_wrapped(|ui| {
            for (setting, key) in [
                (FrequencySetting::EveryNMinutes, "dt-cron-every-minutes"),
                (FrequencySetting::EveryNHours, "dt-cron-every-hours"),
                (FrequencySetting::Daily, "dt-cron-daily"),
                (FrequencySetting::Weekly, "dt-cron-weekly"),
                (FrequencySetting::Monthly, "dt-cron-monthly"),
                (FrequencySetting::Yearly, "dt-cron-yearly"),
            ] {
                ui.selectable_value(&mut st.frequency, setting, s.t(key));
            }
        });
        ui.add_space(6.0);
        match st.frequency {
            FrequencySetting::EveryNMinutes => {
                ui.horizontal(|ui| {
                    ui.label(s.t("dt-cron-interval-minutes"));
                    ui.add(egui::DragValue::new(&mut st.interval).range(1..=59));
                });
            }
            FrequencySetting::EveryNHours => {
                ui.horizontal(|ui| {
                    ui.label(s.t("dt-cron-interval-hours"));
                    ui.add(egui::DragValue::new(&mut st.interval).range(1..=23));
                    ui.label(s.t("dt-cron-at-minute"));
                    ui.add(egui::DragValue::new(&mut st.minute).range(0..=59));
                });
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.label(s.t("dt-cron-at"));
                    ui.add(egui::DragValue::new(&mut st.hour).range(0..=23));
                    ui.label(":");
                    ui.add(egui::DragValue::new(&mut st.minute).range(0..=59));
                });
            }
        }
        if st.frequency == FrequencySetting::Weekly {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for (i, name) in DAY_NAMES.iter().enumerate() {
                    ui.checkbox(&mut st.days[i], *name);
                }
            });
        }
        if matches!(
            st.frequency,
            FrequencySetting::Monthly | FrequencySetting::Yearly
        ) {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(s.t("dt-cron-day-of-month"));
                ui.add(egui::DragValue::new(&mut st.day_of_month).range(1..=31));
                if st.frequency == FrequencySetting::Yearly {
                    ui.label(s.t("dt-cron-month"));
                    egui::ComboBox::from_id_salt("dt-cron-month")
                        .selected_text(MONTH_NAMES[(st.month.clamp(1, 12) - 1) as usize])
                        .show_ui(ui, |ui| {
                            for (i, m) in MONTH_NAMES.iter().enumerate() {
                                ui.selectable_value(&mut st.month, i as u32 + 1, *m);
                            }
                        });
                }
            });
        }
    });
    let schedule = st.schedule();
    ui.add_space(8.0);
    form::section(ui, theme, &s.t("dt-cron-expression"), |ui| {
        form::hint(ui, theme, &describe(&schedule));
        ui.add_space(6.0);
        // All three at once: the usual reason to be here is needing the
        // other dialect's version of the one you already have.
        for flavour in Flavour::ALL {
            match expression(&schedule, flavour) {
                Ok(expr) => {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(flavour.label())
                                .color(theme.text_muted)
                                .small(),
                        );
                        ui.add(
                            egui::Label::new(egui::RichText::new(&expr).monospace())
                                .selectable(true),
                        );
                        if ui
                            .small_button(format!("{} {}", hs_ui::icons::COPY, s.t("dt-copy")))
                            .clicked()
                        {
                            ui.ctx().copy_text(expr.clone());
                        }
                    });
                }
                Err(e) => {
                    ui.colored_label(theme.error, format!("{}: {e}", flavour.label()));
                }
            }
        }
    });
    ui.add_space(8.0);
    form::section(ui, theme, &s.t("dt-cron-check"), |ui| {
        form::hint(ui, theme, &s.t("dt-cron-check-hint"));
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut st.check_input)
                .hint_text("0 30 9 * * ?")
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Monospace),
        );
        if st.check_input.trim().is_empty() {
            return;
        }
        ui.add_space(6.0);
        // One row per dialect: whether it takes the expression, and what
        // it would mean if it does.
        egui::Grid::new("dt-cron-check-grid")
            .num_columns(3)
            .spacing([16.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                for key in [
                    "dt-cron-col-type",
                    "dt-cron-col-match",
                    "dt-cron-col-detail",
                ] {
                    ui.label(
                        egui::RichText::new(s.t(key))
                            .strong()
                            .color(theme.text_muted),
                    );
                }
                ui.end_row();
                for flavour in Flavour::ALL {
                    ui.label(flavour.label());
                    match explain(&st.check_input, flavour) {
                        Ok(detail) => {
                            ui.colored_label(
                                theme.success,
                                format!("{} {}", hs_ui::icons::CHECK, s.t("dt-cron-match")),
                            );
                            ui.label(detail);
                        }
                        Err(e) => {
                            ui.colored_label(theme.text_muted, s.t("dt-cron-no-match"));
                            ui.colored_label(theme.text_muted, e);
                        }
                    }
                    ui.end_row();
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daily() -> Schedule {
        Schedule {
            frequency: Frequency::Daily,
            hour: 9,
            minute: 30,
            ..Default::default()
        }
    }

    #[test]
    fn the_dialects_differ_where_they_should() {
        let s = daily();
        assert_eq!(expression(&s, Flavour::Unix).unwrap(), "30 9 * * *");
        // Spring and Quartz lead with seconds.
        assert_eq!(expression(&s, Flavour::Spring).unwrap(), "0 30 9 * * *");
        // Quartz will not take * in both day fields.
        assert_eq!(expression(&s, Flavour::Quartz).unwrap(), "0 30 9 * * ?");
    }

    #[test]
    fn every_n_minutes_and_hours() {
        let s = Schedule {
            frequency: Frequency::EveryNMinutes,
            interval: 15,
            ..Default::default()
        };
        assert_eq!(expression(&s, Flavour::Unix).unwrap(), "*/15 * * * *");
        let s = Schedule {
            frequency: Frequency::EveryNHours,
            interval: 6,
            minute: 5,
            ..Default::default()
        };
        assert_eq!(expression(&s, Flavour::Unix).unwrap(), "5 */6 * * *");
        assert_eq!(expression(&s, Flavour::Spring).unwrap(), "0 5 */6 * * *");
    }

    #[test]
    fn weekly_lists_the_days_and_quartz_blanks_the_other_field() {
        let mut s = daily();
        s.frequency = Frequency::Weekly;
        s.days = [false, true, false, true, false, true, false];
        assert_eq!(expression(&s, Flavour::Unix).unwrap(), "30 9 * * 1,3,5");
        assert_eq!(
            expression(&s, Flavour::Quartz).unwrap(),
            "0 30 9 ? * 1,3,5",
            "Quartz needs ? in day-of-month when a weekday is given"
        );
        s.days = [false; 7];
        assert!(expression(&s, Flavour::Unix).is_err());
    }

    #[test]
    fn monthly_and_yearly() {
        let mut s = daily();
        s.frequency = Frequency::Monthly;
        s.day_of_month = 15;
        assert_eq!(expression(&s, Flavour::Unix).unwrap(), "30 9 15 * *");
        assert_eq!(expression(&s, Flavour::Quartz).unwrap(), "0 30 9 15 * ?");
        s.frequency = Frequency::Yearly;
        s.month = 3;
        assert_eq!(expression(&s, Flavour::Unix).unwrap(), "30 9 15 3 *");
    }

    #[test]
    fn impossible_values_are_refused_with_a_reason() {
        let bad = Schedule {
            minute: 60,
            ..daily()
        };
        assert!(
            expression(&bad, Flavour::Unix)
                .unwrap_err()
                .contains("minute")
        );
        let bad = Schedule {
            hour: 24,
            ..daily()
        };
        assert!(
            expression(&bad, Flavour::Unix)
                .unwrap_err()
                .contains("hour")
        );
        let bad = Schedule {
            frequency: Frequency::EveryNMinutes,
            interval: 0,
            ..daily()
        };
        assert!(expression(&bad, Flavour::Unix).is_err());
    }

    #[test]
    fn a_schedule_describes_itself() {
        assert_eq!(describe(&daily()), "Every day at 09:30.");
        let s = Schedule {
            frequency: Frequency::EveryNMinutes,
            interval: 1,
            ..daily()
        };
        assert_eq!(describe(&s), "Every minute.");
        let mut s = daily();
        s.frequency = Frequency::Weekly;
        s.days = [false, true, false, false, false, false, false];
        assert_eq!(describe(&s), "Every Mon at 09:30.");
        let mut s = daily();
        s.frequency = Frequency::Yearly;
        s.day_of_month = 25;
        s.month = 12;
        assert_eq!(describe(&s), "On 25 December every year at 09:30.");
    }

    #[test]
    fn checking_counts_the_fields_for_the_dialect() {
        assert!(check("30 9 * * *", Flavour::Unix).is_ok());
        assert!(check("0 30 9 * * ?", Flavour::Quartz).is_ok());
        assert!(check("0 30 9 * * *", Flavour::Spring).is_ok());
        // A Unix expression pasted into Spring has one field too few.
        let e = check("30 9 * * *", Flavour::Spring).unwrap_err();
        assert!(e.contains("6 fields"), "{e}");
        assert!(e.contains("has 5"), "{e}");
    }

    #[test]
    fn checking_catches_values_out_of_range() {
        assert!(
            check("61 9 * * *", Flavour::Unix)
                .unwrap_err()
                .contains("minute")
        );
        assert!(
            check("30 25 * * *", Flavour::Unix)
                .unwrap_err()
                .contains("hour")
        );
        assert!(
            check("30 9 0 * *", Flavour::Unix)
                .unwrap_err()
                .contains("day")
        );
        assert!(
            check("30 9 * 13 *", Flavour::Unix)
                .unwrap_err()
                .contains("month")
        );
        // Lists, ranges and steps are all read.
        assert!(check("0,15,30,45 9-17 * * 1-5", Flavour::Unix).is_ok());
        assert!(check("*/5 * * * *", Flavour::Unix).is_ok());
        assert!(check("*/0 * * * *", Flavour::Unix).is_err());
        // Names are passed through rather than guessed at.
        assert!(check("0 9 * JAN MON", Flavour::Unix).is_ok());
        assert!(check("0 0 L * ?", Flavour::Quartz).is_err(), "five fields");
    }

    #[test]
    fn quartz_insists_on_exactly_one_question_mark() {
        assert!(check("0 30 9 * * *", Flavour::Quartz).is_err());
        assert!(check("0 30 9 ? * ?", Flavour::Quartz).is_err());
        assert!(check("0 30 9 15 * ?", Flavour::Quartz).is_ok());
        assert!(check("0 30 9 ? * MON", Flavour::Quartz).is_ok());
    }

    #[test]
    fn an_expression_explains_itself() {
        assert_eq!(
            explain("30 9 * * *", Flavour::Unix).unwrap(),
            "At 09:30, every day."
        );
        assert_eq!(
            explain("*/15 * * * *", Flavour::Unix).unwrap(),
            "Every 15 minutes, every day."
        );
        assert_eq!(
            explain("0 30 9 ? * 1,3,5", Flavour::Quartz).unwrap(),
            "At 09:30, on Mon, Wed, Fri."
        );
        assert_eq!(
            explain("0 0 1 7 *", Flavour::Unix).unwrap(),
            "At 00:00, on day 1 of the month, in July."
        );
        // Something it cannot phrase is named, not guessed at.
        let e = explain("0 0 0 ? * 2#1", Flavour::Quartz).unwrap();
        assert!(e.contains("2#1"), "{e}");
        // A bad expression explains nothing.
        assert!(explain("61 9 * * *", Flavour::Unix).is_err());
    }

    #[test]
    fn every_built_expression_can_be_explained() {
        for f in Flavour::ALL {
            let e = expression(&daily(), f).unwrap();
            let text = explain(&e, f)
                .unwrap_or_else(|err| panic!("{f:?} built {e:?} and could not explain it: {err}"));
            assert!(text.contains("09:30"), "{f:?}: {text}");
        }
    }

    #[test]
    fn what_is_built_passes_its_own_check() {
        let schedules = [
            daily(),
            Schedule {
                frequency: Frequency::EveryNMinutes,
                interval: 10,
                ..daily()
            },
            Schedule {
                frequency: Frequency::EveryNHours,
                interval: 2,
                ..daily()
            },
            Schedule {
                frequency: Frequency::Weekly,
                ..daily()
            },
            Schedule {
                frequency: Frequency::Monthly,
                day_of_month: 28,
                ..daily()
            },
            Schedule {
                frequency: Frequency::Yearly,
                day_of_month: 1,
                month: 7,
                ..daily()
            },
        ];
        for s in &schedules {
            for f in Flavour::ALL {
                let e = expression(s, f).unwrap();
                check(&e, f)
                    .unwrap_or_else(|err| panic!("{f:?} built {e:?} and then refused it: {err}"));
            }
        }
    }
}
