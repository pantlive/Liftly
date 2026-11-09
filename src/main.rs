#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(any(windows, test))]
mod autostart;
#[cfg(any(windows, test))]
mod config;
#[cfg(any(windows, test))]
mod schedule;
#[cfg(any(windows, test))]
mod view;

#[cfg(test)]
#[path = "../build/rsrc.rs"]
mod rsrc;

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
