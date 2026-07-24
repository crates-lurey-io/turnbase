//! Browser demo entry for Risk: a 3-seat all-bot match on the shared dashboard.

use risk::Risk;
use turnbase_demos::{Demo, dashboard, demo_entry};
use turnbase_simulator::SimulationRunner;

#[derive(Default)]
struct RiskDemo;

impl Demo for RiskDemo {
    type App = SimulationRunner<Risk>;

    fn build(seed: u64) -> Self::App {
        dashboard(Risk::new(3), seed)
    }

    fn is_over(app: &Self::App) -> bool {
        app.is_terminal()
    }
}

demo_entry!(RiskDemo);
