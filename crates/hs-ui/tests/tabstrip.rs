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
        split_right: "Split right".into(),
        split_down: "Split down".into(),
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
                visible: &[],
                labels: &labels,
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
