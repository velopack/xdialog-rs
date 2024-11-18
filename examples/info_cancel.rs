fn main() {
    println!("Starting Example...");
    xdialog::XDialogBuilder::new()
        // .with_backend(xdialog::XDialogBackend::XamlIsland)
        .run_loop(run);
}

fn run() -> i32 {
    println!("Showing OK/Cancel Dialog!");
    let result =
        xdialog::show_message_ok_cancel("Title", "Main instruction", "Are you happy with things?", xdialog::XDialogIcon::Information)
            .unwrap();

    if result {
        println!("OK button pressed")
    } else {
        println!("Cancel button pressed")
    }

    return 0;
}
