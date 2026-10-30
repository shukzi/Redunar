//! The production pre-GTK selection function, exercised in a private display.
#[path = "../src/display_backend.rs"]
mod display_backend;

use gtk::prelude::*;

fn main() {
    assert_eq!(
        std::env::var("REDUNAR_PRIVATE_DESKTOP_FIXTURE").as_deref(),
        Ok("1")
    );
    let inherited = std::env::var_os("GDK_BACKEND");
    display_backend::configure();
    assert_eq!(
        std::env::var_os("GDK_BACKEND"),
        inherited,
        "backend preference must not leak into child environment"
    );
    gtk::init().expect("private GTK display must be reachable");
    let display = gtk::gdk::Display::default().unwrap();
    let actual = display.type_().name();
    assert_eq!(
        Some(actual),
        std::env::var("REDUNAR_EXPECT_DISPLAY_TYPE").ok().as_deref()
    );
    display_backend::record_selected();
    println!("PASS: selected {actual}; caller environment unchanged");
}
