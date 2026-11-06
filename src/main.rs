#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(any(windows, test))]
mod config;
#[cfg(any(windows, test))]
mod schedule;

#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("仅支持 Windows");
    std::process::exit(1);
}
