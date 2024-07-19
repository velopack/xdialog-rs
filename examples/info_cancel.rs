fn main() {
    println!("Showing OK/Cancel Dialog!");
    xdialog::XDialogBuilder::new().run(run);
}

fn run() -> u16 {
    let result = xdialog::show_message_box_info_ok_cancel(
        "Title",
        "Main instruction",
        "Are you happy with things?").unwrap();

    if result {
        println!("OK button pressed")
    } else {
        println!("Cancel button pressed")
    }

    return 0;
}
