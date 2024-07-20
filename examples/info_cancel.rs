fn main() {
    println!("Showing OK/Cancel Dialog!");
    xdialog::XDialogBuilder::new().run(run);
}

fn run() -> i32 {
    let result = xdialog::show_message_ok_cancel(
        "Title",
        "Main instruction",
        "Are you happy with things?",
        xdialog::XDialogIcon::Information,
    ).unwrap();

    if result {
        println!("OK button pressed")
    } else {
        println!("Cancel button pressed")
    }

    return 0;
}
