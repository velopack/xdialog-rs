fn main() {
    println!("Showing OK/Cancel Dialog!");
    xdialog::run(run)
}

fn run() -> u16 {
    let result = xdialog::show_message_box_info_ok_cancel(
        "Title",
        "Main instruction",
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.").unwrap();

    if result {
        println!("OK button pressed")
    } else {
        println!("Cancel button pressed")
    }

    return 0;
}
