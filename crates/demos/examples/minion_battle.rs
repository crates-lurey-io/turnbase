//! Browser demo entry for Minion Battle: an all-bot match on the shared dashboard.

use minion_battle::MinionBattle;
use turnbase_demos::{Demo, dashboard, demo_entry};
use turnbase_simulator::{SessionApp, standard_bots};

#[derive(Default)]
struct MinionBattleDemo;

impl Demo for MinionBattleDemo {
    type App = SessionApp<MinionBattle>;

    fn build(seed: u64) -> Self::App {
        dashboard(MinionBattle, standard_bots(), seed)
    }

    fn is_over(app: &Self::App) -> bool {
        app.is_terminal()
    }
}

demo_entry!(MinionBattleDemo);
