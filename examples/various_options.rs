use xdialog::{show_progress, XDialogIcon};

fn main() {
    xdialog::XDialogBuilder::new().run_loop(run);
}

fn run() -> i32 {
    let long_instruction = "This is v. long main instruction which will almost certainly need to wrap into several lines and I need to make sure that the dialog sizes correctly";
    let small_text = "This is a very small dialog message!";
    let medium_text = "At vero eos et accusamus et iusto odio dignissimos ducimus qui blanditiis praesentium voluptatum deleniti atque corrupti quos dolores et quas molestias excepturi sint occaecati cupiditate non provident, similique sunt in culpa qui officia deserunt mollitia animi, id est laborum et dolorum fuga. Et harum quidem rerum facilis est et expedita distinctio. Nam libero tempore, cum soluta nobis est eligendi optio cumque nihil impedit quo minus id quod maxime placeat facere possimus, omnis voluptas assumenda est, omnis dolor repellendus. Temporibus autem quibusdam et aut officiis debitis aut rerum necessitatibus saepe eveniet ut et voluptates repudiandae sint et molestiae non recusandae. Itaque earum rerum hic tenetur a sapiente delectus, ut aut reiciendis voluptatibus maiores alias consequatur aut perferendis doloribus asperiores repellat.";
    let _ = xdialog::show_message_ok_cancel("Title", "Main instruction", small_text, XDialogIcon::Information).unwrap();
    let _ = xdialog::show_message_ok_cancel("Title", "Main instruction", medium_text, XDialogIcon::Information).unwrap();

    let mut data = xdialog::XDialogOptions {
        icon: XDialogIcon::None,
        message: small_text.to_string(),
        buttons: vec!["OK".to_string()],
        main_instruction: "This is a main instruction".to_string(),
        title: "This is a title".to_string(),
    };
    let _ = xdialog::show_message(data.clone());

    data.message = medium_text.to_string();
    let _ = xdialog::show_message(data.clone());

    data.message = small_text.to_string();
    data.main_instruction = long_instruction.to_string();
    let _ = xdialog::show_message(data.clone());

    data.icon = XDialogIcon::Error;
    let _ = xdialog::show_message(data.clone());

    data.message = medium_text.to_string();
    data.title = "".to_string();
    let _ = xdialog::show_message(data.clone());

    let d = show_progress("Title", "This is an instruction", small_text, XDialogIcon::None).unwrap();
    d.set_indeterminate().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3));
    d.close().unwrap();

    let d = show_progress("Title", "", medium_text, XDialogIcon::None).unwrap();
    d.set_indeterminate().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3));
    d.close().unwrap();

    let d = show_progress("Title", long_instruction, medium_text, XDialogIcon::Error).unwrap();
    d.set_indeterminate().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3));
    d.close().unwrap();

    let d = show_progress("Title", "", small_text, XDialogIcon::Error).unwrap();
    d.set_indeterminate().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3));
    d.close().unwrap();

    return 0;
}
