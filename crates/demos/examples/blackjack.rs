//! Browser demo entry for Blackjack: an all-bot match on the game's own
//! bespoke `BlackjackTui`, not the shared dashboard.

use blackjack::Blackjack;
use blackjack::tui::BlackjackTui;
use turnbase_demos::{Demo, all_bots, demo_entry};

#[derive(Default)]
struct BlackjackDemo;

impl Demo for BlackjackDemo {
    type App = BlackjackTui;

    fn build(seed: u64) -> Self::App {
        // with_builder (not new) so the demo's reset key can rebuild the match.
        BlackjackTui::with_builder(seed, |seed| all_bots(Blackjack::default(), seed))
    }

    fn is_over(app: &Self::App) -> bool {
        app.is_terminal()
    }
}

demo_entry!(BlackjackDemo);
