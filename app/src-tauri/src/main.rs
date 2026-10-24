// Release builds are GUI programs: no console window opens on double-click.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ardoise_app::run();
}
