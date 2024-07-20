fn main() {
    xdialog::XDialogBuilder::new().run(run);
}

fn run() -> i32 {
    let result = xdialog::show_progress_dialog(
        xdialog::XDialogIcon::Warning,
        "My App Incorporated",
        "Doing some hard thing",
        "Solving string theory...").unwrap();

    std::thread::sleep(std::time::Duration::from_secs(1));
    result.set_value(0.2).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));
    result.set_value(0.4).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));
    result.set_value(0.6).unwrap();
    result.set_text("This is some long text which should wrap and cause the window size to be re-calculated.").unwrap();
    std::thread::sleep(std::time::Duration::from_secs(2));
    result.set_value(0.8).unwrap();
    result.set_text("Almost done...").unwrap();
    std::thread::sleep(std::time::Duration::from_secs(2));
    result.set_value(1.0).unwrap();
    result.set_text("Full progress bar anyone?").unwrap();
    std::thread::sleep(std::time::Duration::from_secs(2));
    result.set_text("Oops, not quite there yet.").unwrap();
    result.set_indeterminate().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(5));
    return 0;
}
