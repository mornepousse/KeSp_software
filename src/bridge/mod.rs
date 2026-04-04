mod connection;
mod keymap;
mod key_selector;
mod advanced;
mod macros;
mod stats;
mod settings;

use crate::msg::AppShared;
use crate::MainWindow;
#[allow(unused_imports)]
use slint::ComponentHandle;

/// Wire up all Slint bridge callbacks. Called once from main().
pub fn setup_all(window: &MainWindow, shared: &AppShared) {
    connection::setup(window, shared);
    keymap::setup(window, shared);
    key_selector::setup(window, shared);
    advanced::setup(window, shared);
    macros::setup(window, shared);
    stats::setup(window, shared);
    settings::setup(window, shared);
}
