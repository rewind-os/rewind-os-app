// Release builds are GUI programs: no console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    match rewind_os_ui::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Rewind OS: {err}");
            ExitCode::FAILURE
        }
    }
}
