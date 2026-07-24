//! Browser demo entry for Coup: a 4-seat all-bot match on the shared dashboard.

use coup::Coup;
use turnbase_demos::{Demo, dashboard, demo_entry};
use turnbase_simulator::SimulationRunner;

#[derive(Default)]
struct CoupDemo;

impl Demo for CoupDemo {
    type App = SimulationRunner<Coup>;

    fn build(seed: u64) -> Self::App {
        dashboard(Coup::new(4), seed)
    }

    fn is_over(app: &Self::App) -> bool {
        app.is_terminal()
    }
}

demo_entry!(CoupDemo);
