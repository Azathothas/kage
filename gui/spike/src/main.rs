//! Binary entry for the egui spike probe.

fn main() -> eframe::Result {
    spike::run_native(spike::options_from_args())
}
