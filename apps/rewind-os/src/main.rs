// Release builds are GUI programs: no console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    // Started by the scheduler: clean the scheduled categories with no window, and exit.
    if std::env::args().any(|a| a == rewind_os_engine::schedule::APP_FLAG) {
        let task = rewind_os_engine::Task::new();
        return match rewind_os_engine::schedule::run(&task) {
            Ok(_) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("Rewind OS: {err}");
                ExitCode::FAILURE
            }
        };
    }
    match rewind_os_ui::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Rewind OS: {err}");
            ExitCode::FAILURE
        }
    }
}
