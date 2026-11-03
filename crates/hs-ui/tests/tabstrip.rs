//! Headless interaction test of the tab strip with egui_kittest (no GPU, no
//! image snapshots: the runner has no graphics device).

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use hs_ui::tabs::{TabItem, TabStrip, TabStripLabels, TabStripResponse};
use hs_ui::theme::Theme;

struct T {
    id: u64,
    title: String,
    pinned: bool,
}

impl TabItem for T {
    fn id(&self) -> u64 {
        self.id
    }
    fn title(&self) -> String {
        self.title.clone()
    }
    fn icon(&self) -> &str {
        "•"
    }
    fn pinned(&self) -> bool {
        self.pinned
    }
}

fn labels() -> TabStripLabels {
    TabStripLabels {
        new_tab: "New tab".into(),
        close: "Close".into(),
        close_others: "Close others".into(),
        close_left: "Close left".into(),
        close_right: "Close right".into(),
        pin: "Pin".into(),
        unpin: "Unpin".into(),
        duplicate: "Duplicate".into(),
        duplicate_blocked: "Already open".into(),
        copy_path: "Copy path".into(),
    }
}

#[derive(Default)]
struct State {
    last: TabStripResponse,
    active: Option<u64>,
    new_tab_seen: bool,
}

#[test]
fn clicking_the_plus_button_requests_a_new_tab_and_strip_wraps() {
    let theme = Theme::dark();
    let labels = labels();
    let tabs: Vec<T> = (1..=12)
        .map(|i| T {
            id: i,
            title: format!("Tab {i}"),
            pinned: i == 1,
        })
        .collect();
    let mut harness = Harness::new_ui_state(
        |ui, state: &mut State| {
            ui.set_width(700.0);
            let strip = TabStrip {
                theme: &theme,
                min_width: 140.0,
                gap: 4.0,
                active: state.active,
                labels: &labels,
                compact: false,
                show_new_tab: true,
                height: None,
            };
            let r = strip.show(ui, &tabs);
            if let Some(id) = r.activated {
                state.active = Some(id);
            }
            if r.new_tab {
                state.new_tab_seen = true;
            }
            state.last = r;
        },
        State::default(),
    );
    harness.run();
    // 11 regular tabs at ≥140 px cannot fit in 700 px → more than one row.
    let rows = hs_ui::tabs::tab_row_count(
        &tabs.iter().map(|t| t.pinned).collect::<Vec<_>>(),
        &vec![false; tabs.len()],
        &vec![false; tabs.len()],
        700.0,
        140.0,
        4.0,
    );
    assert!(rows >= 3, "expected wrapping, got {rows} rows");

    harness.get_by_label("New tab").click();
    harness.run();
    assert!(
        harness.state().new_tab_seen,
        "clicking + should request a new tab"
    );
}

/// Which "close the others" entries each position offers (CR-0029).
#[test]
fn a_tabs_close_menu_only_offers_what_it_would_close() {
    use hs_ui::tabs::close_scopes;

    // The activity's permanent tab, then three closable ones.
    let scopes = close_scopes(&[false, true, true, true]);
    // The permanent tab: Close Others closes the three, and Close to the
    // Right would close exactly the same three, so it is not offered.
    assert!(scopes[0].others());
    assert!(!scopes[0].close_right());
    assert!(!scopes[0].close_left());
    // The first closable tab: nothing to its left.
    assert!(scopes[1].others());
    assert!(!scopes[1].close_left());
    assert!(!scopes[1].close_right());
    // The middle one has tabs on both sides, so both directions mean
    // something different from Close Others.
    assert!(scopes[2].close_left());
    assert!(scopes[2].close_right());
    // The last one: Close to the Left would be Close Others again.
    assert!(scopes[3].others());
    assert!(!scopes[3].close_left());
    assert!(!scopes[3].close_right());

    // A permanent tab on its own offers nothing at all.
    let alone = close_scopes(&[false]);
    assert!(!alone[0].others());

    // Pinned tabs survive all three, so they are not neighbours worth
    // closing: one pinned tab beside one closable one is "alone" too.
    let pinned_only = close_scopes(&[false, false]);
    assert!(!pinned_only[1].others());

    // A tab that is not closable is drawn in the lead zone at the head
    // of the strip, so every closable tab is to its right wherever it
    // sits in the list - a pinned tab late in the list still offers
    // Close to the Right and never Close to the Left.
    let late = close_scopes(&[true, true, false]);
    assert!(late[2].right, "the closable tabs are drawn to its right");
    assert!(!late[2].left, "and none of them to its left");
}
