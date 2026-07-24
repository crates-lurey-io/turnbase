//! Browser demo entry for Minion Battle: an all-bot match on the shared dashboard.

use minion_battle::MinionBattle;
use turnbase_demos::{Demo, dashboard, demo_entry};
use turnbase_simulator::SimulationRunner;

#[derive(Default)]
struct MinionBattleDemo;

impl Demo for MinionBattleDemo {
    type App = SimulationRunner<MinionBattle>;

    fn build(seed: u64) -> Self::App {
        dashboard(MinionBattle, seed)
    }

    fn is_over(app: &Self::App) -> bool {
        app.is_terminal()
    }
}

demo_entry!(MinionBattleDemo);
