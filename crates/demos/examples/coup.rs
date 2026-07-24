//! Browser demo entry for Coup: a 4-seat all-bot match on the shared dashboard.

use coup::Coup;
use turnbase_demos::{Demo, dashboard, demo_entry};
use turnbase_simulator::{SessionApp, ismcts_bot, mcts_bot, random_bot};

#[derive(Default)]
struct CoupDemo;

impl Demo for CoupDemo {
    type App = SessionApp<Coup>;

    fn build(seed: u64) -> Self::App {
        // Coup implements Determinize, so ISMCTS is on offer alongside MCTS.
        dashboard(
            Coup::new(4),
            vec![random_bot(), mcts_bot(), ismcts_bot()],
            seed,
        )
    }

    fn is_over(app: &Self::App) -> bool {
        app.is_terminal()
    }
}

demo_entry!(CoupDemo);
