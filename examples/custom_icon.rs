//! A message and a progress dialog with a custom icon (egui backends):
//! `cargo run --example custom_icon -- <path to .ico/.png/.icns>`.

use xdialog::*;

fn main() {
    XDialogBuilder::new().run(run);
}

fn run() {
    // Or `XDialogIconSource::Bytes(include_bytes!("app.ico").as_slice().into())`.
    let icon_source = std::env::args_os().nth(1).map(|path| XDialogIconSource::File(path.into()));
    if icon_source.is_none() {
        println!("No icon file given: the Custom icon shows nothing.");
    }
    let options = XDialogOptions { title: "My App".into(),
                                   main_instruction: "Update available".into(),
                                   message: "Version 2.0 is ready to install.".into(),
                                   icon: XDialogIcon::Custom,
                                   icon_source: icon_source.clone(),
                                   buttons: vec!["Later".into(), "Install".into()] };
    if show_message(options).wait().unwrap() != XDialogResult::ButtonPressed(1) {
        return;
    }

    let progress = show_progress_ex(XDialogOptions { title: "My App".into(),
                                                     main_instruction: "Installing".into(),
                                                     message: "Downloading...".into(),
                                                     icon: XDialogIcon::Custom,
                                                     icon_source,
                                                     buttons: vec![] }).unwrap();
    for i in 0..=20 {
        progress.set_value(i as f32 / 20.0).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    progress.close().unwrap();
}
