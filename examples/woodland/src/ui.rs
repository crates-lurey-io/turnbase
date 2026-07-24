//! The retroglyph dashboard rendering for Woodland (the `ui` feature).
//!
//! No hidden information, so the viewport renders the whole ring: each
//! clearing's Marquise warriors/building and Alliance warriors/sympathy
//! token, both factions' victory points, and whose turn it is with the
//! actions remaining. [`format_action`](PrintableGame::format_action) renders
//! both halves of [`Action`] -- the enum-of-enums axis -- so a Marquise move
//! reads `"A: ..."` and an Alliance move reads `"B: ..."`.

use retroglyph_core::grid::Rect;
use retroglyph_core::{AnsiColor, Backend, Color, Terminal};
use turnbase_simulator::PrintableGame;

use crate::{Action, AllianceAction, CLEARINGS, Faction, MarquiseAction, Woodland, WoodlandView};

impl PrintableGame for Woodland {
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect) {
        let mut y = area.top();
        term.print(area.left(), y, "== Woodland ==");
        y = y.saturating_add(2);

        term.fg(Color::Ansi(AnsiColor::BrightYellow));
        term.print(
            area.left(),
            y,
            &format!(
                "{} to move, {} action(s) left",
                describe_faction(view.current),
                view.actions_left
            ),
        );
        term.reset_style();
        y = y.saturating_add(2);

        for clearing in 0..CLEARINGS {
            term.print(area.left(), y, &describe_clearing(view, clearing));
            y = y.saturating_add(1);
        }
        y = y.saturating_add(1);

        let marquise_deciding = view.current == Faction::Marquise;
        if marquise_deciding {
            term.fg(Color::Ansi(AnsiColor::BrightGreen));
        }
        term.print(
            area.left(),
            y,
            &format!(
                "{} A (Marquise): {} vp",
                if marquise_deciding { '>' } else { ' ' },
                view.vp_marquise
            ),
        );
        term.reset_style();
        y = y.saturating_add(1);

        if !marquise_deciding {
            term.fg(Color::Ansi(AnsiColor::BrightGreen));
        }
        term.print(
            area.left(),
            y,
            &format!(
                "{} B (Alliance): {} vp",
                if marquise_deciding { ' ' } else { '>' },
                view.vp_alliance
            ),
        );
        term.reset_style();
    }

    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)> {
        vec![
            ("vp A".to_owned(), view.vp_marquise.to_string()),
            ("vp B".to_owned(), view.vp_alliance.to_string()),
            ("turn".to_owned(), describe_faction(view.current)),
            ("actions left".to_owned(), view.actions_left.to_string()),
        ]
    }

    fn format_action(&self, action: &Self::Action) -> String {
        match action {
            Action::Marquise(a) => format!("A: {}", describe_marquise_action(*a)),
            Action::Alliance(a) => format!("B: {}", describe_alliance_action(*a)),
        }
    }
}

/// Short faction label for the stats/turn line.
fn describe_faction(faction: Faction) -> String {
    match faction {
        Faction::Marquise => "A (Marquise)".to_owned(),
        Faction::Alliance => "B (Alliance)".to_owned(),
    }
}

/// One line for `clearing`: Marquise warriors/building, then Alliance
/// warriors/sympathy.
fn describe_clearing(view: &WoodlandView, clearing: usize) -> String {
    let building = if view.buildings[clearing] { "+bld" } else { "" };
    let sympathy = if view.sympathy[clearing] { "+sym" } else { "" };
    format!(
        "c{clearing}: A {}w{building}   B {}w{sympathy}",
        view.warriors_marquise[clearing], view.warriors_alliance[clearing]
    )
}

/// Renders a Marquise action for the action-select menu.
fn describe_marquise_action(action: MarquiseAction) -> String {
    match action {
        MarquiseAction::March(from, to) => format!("march c{from}->c{to}"),
        MarquiseAction::Build(clearing) => format!("build @c{clearing}"),
        MarquiseAction::Recruit(clearing) => format!("recruit @c{clearing}"),
        MarquiseAction::EndTurn => "end turn".to_owned(),
    }
}

/// Renders an Alliance action for the action-select menu.
fn describe_alliance_action(action: AllianceAction) -> String {
    match action {
        AllianceAction::Spread(clearing) => format!("spread @c{clearing}"),
        AllianceAction::Organize(clearing) => format!("organize @c{clearing}"),
        AllianceAction::Move(from, to) => format!("move c{from}->c{to}"),
        AllianceAction::EndTurn => "end turn".to_owned(),
    }
}
