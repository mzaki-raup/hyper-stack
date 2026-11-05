//! A date field: the ISO date as text, with a calendar behind a button for
//! picking one (FEAT-0095). The calendar is drawn here rather than taken
//! from `egui_extras`, whose picker pulls in a second date library: the
//! civil-calendar arithmetic it needs is a dozen lines, and the servers the
//! field talks to (Redmine, Jira, GitLab) all take and give `YYYY-MM-DD`.

use egui::Vec2;

use crate::form::{self, FIELD_MAX_WIDTH, FIELD_MIN_WIDTH};
use crate::icons::ph;
use crate::theme::Theme;
use crate::tip::TipExt;

/// The calendar's words, from the caller's locale.
pub struct DateLabels<'a> {
    /// Tooltip of the calendar button.
    pub open: &'a str,
    pub today: &'a str,
    pub clear: &'a str,
    /// January … December.
    pub months: &'a [String],
    /// Monday … Sunday, short.
    pub weekdays: &'a [String],
}

/// A `YYYY-MM-DD` text field with a calendar button beside it. `value`
/// stays text, so a date can be typed, pasted or cleared; picking a day
/// writes it in the server's format. Returns the text field's response,
/// marked changed when the calendar wrote the value.
pub fn date_field(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    value: &mut String,
    labels: &DateLabels<'_>,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let w = (ui.available_width().clamp(FIELD_MIN_WIDTH, FIELD_MAX_WIDTH) - 32.0).max(80.0);
        let mut r = form::text_field_width(ui, value, "YYYY-MM-DD", w);
        let button = form::icon_button(ui, theme, ph::CALENDAR_BLANK, 14.0, false).tip(labels.open);
        let id = ui.make_persistent_id(("date-field", id_salt));
        let mut picked = false;
        egui::Popup::menu(&button).show(|ui| {
            picked = calendar(ui, theme, id, value, labels);
        });
        if picked {
            r.mark_changed();
        }
        r
    })
    .inner
}

/// The month grid: a header with the month and arrows, the weekday
/// initials, and a button per day. The month on view is kept in egui's
/// temporary memory, starting at the field's date or today. Returns
/// whether `value` was written.
fn calendar(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: egui::Id,
    value: &mut String,
    labels: &DateLabels<'_>,
) -> bool {
    let mut wrote = false;
    let today = today();
    let current = parse_iso(value);
    let (mut year, mut month) = ui
        .data(|d| d.get_temp::<(i32, u32)>(id))
        .unwrap_or_else(|| {
            let (y, m, _) = current.unwrap_or(today);
            (y, m)
        });
    ui.set_min_width(7.0 * 30.0 + 8.0);
    ui.horizontal(|ui| {
        if ui.small_button(ph::CARET_LEFT).clicked() {
            (year, month) = if month == 1 {
                (year - 1, 12)
            } else {
                (year, month - 1)
            };
        }
        ui.with_layout(
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                let name = labels
                    .months
                    .get((month - 1) as usize)
                    .map(String::as_str)
                    .unwrap_or("");
                ui.strong(format!("{name} {year}"));
            },
        );
    });
    // The next-month arrow sits hard right of the same header line.
    let header = ui.min_rect();
    let next_rect = egui::Rect::from_center_size(
        egui::pos2(header.right() - 12.0, header.top() + 10.0),
        Vec2::splat(20.0),
    );
    let next = ui.put(next_rect, egui::Button::new(ph::CARET_RIGHT).small());
    if next.clicked() {
        (year, month) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
    }
    ui.data_mut(|d| d.insert_temp(id, (year, month)));
    ui.add_space(4.0);
    egui::Grid::new(id.with("grid"))
        .spacing([2.0, 2.0])
        .min_col_width(28.0)
        .show(ui, |ui| {
            for i in 0..7 {
                let d = labels.weekdays.get(i).map(String::as_str).unwrap_or("");
                ui.add_sized(
                    [28.0, 18.0],
                    egui::Label::new(egui::RichText::new(d).small().color(theme.text_muted)),
                );
            }
            ui.end_row();
            let first = weekday(year, month, 1);
            let days = days_in_month(year, month);
            let mut col = 0;
            for _ in 0..first {
                ui.label("");
                col += 1;
            }
            for day in 1..=days {
                let is_today = (year, month, day) == today;
                let picked = current == Some((year, month, day));
                let text = egui::RichText::new(day.to_string()).color(if is_today && !picked {
                    theme.accent
                } else {
                    theme.text
                });
                let r = ui.add_sized(
                    [28.0, 24.0],
                    egui::Button::new(text)
                        .selected(picked)
                        .frame(picked)
                        .frame_when_inactive(picked),
                );
                if r.clicked() {
                    *value = iso_date(year, month, day);
                    wrote = true;
                    ui.close();
                }
                col += 1;
                if col == 7 {
                    ui.end_row();
                    col = 0;
                }
            }
        });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.small_button(labels.today).clicked() {
            *value = iso_date(today.0, today.1, today.2);
            wrote = true;
            ui.close();
        }
        if ui.small_button(labels.clear).clicked() {
            value.clear();
            wrote = true;
            ui.close();
        }
    });
    wrote
}

/// `YYYY-MM-DD`, zero-padded: the form every tracker the field serves
/// takes a due date in.
pub fn iso_date(year: i32, month: u32, day: u32) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

/// A `YYYY-MM-DD` text as (year, month, day); anything else is `None`.
pub fn parse_iso(text: &str) -> Option<(i32, u32, u32)> {
    let mut parts = text.trim().splitn(3, '-');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    ((1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)).then_some((y, m, d))
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count since 1970-01-01.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

fn days_in_month(y: i32, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1)) as u32
}

/// Monday = 0 … Sunday = 6. 1970-01-01 was a Thursday.
fn weekday(y: i32, m: u32, d: u32) -> u32 {
    (days_from_civil(y, m, d) + 3).rem_euclid(7) as u32
}

fn today() -> (i32, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    civil_from_days(secs.div_euclid(86_400))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calendar_arithmetic_agrees_with_the_civil_calendar() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(days_from_civil(2026, 11, 5)), (2026, 11, 5));
        assert_eq!(weekday(2026, 11, 5), 3, "a Thursday");
        assert_eq!(weekday(2024, 2, 29), 3, "a Thursday too");
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    #[test]
    fn a_picked_day_is_written_in_the_servers_format_and_read_back() {
        // FEAT-0095: the field formats to the server's `YYYY-MM-DD`.
        assert_eq!(iso_date(2026, 3, 7), "2026-03-07");
        assert_eq!(parse_iso("2026-03-07"), Some((2026, 3, 7)));
        assert_eq!(parse_iso(" 2026-11-05 "), Some((2026, 11, 5)));
        assert_eq!(parse_iso("2026-02-30"), None);
        assert_eq!(parse_iso("05/11/2026"), None);
        assert_eq!(parse_iso(""), None);
    }
}
