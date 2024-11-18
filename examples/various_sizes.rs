use xdialog::XDialogIcon;

fn main() {
    println!("Showing OK/Cancel Dialog!");
    xdialog::XDialogBuilder::new().run_loop(run);
}

fn run() -> i32 {
    let _ = xdialog::show_message_ok_cancel(
        "Title",
        "Main instruction",
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.",
        XDialogIcon::Information,
    ).unwrap();

    let _ = xdialog::show_message_ok_cancel(
        "Title",
        "Main instruction",
        "At vero eos et accusamus et iusto odio dignissimos ducimus qui blanditiis praesentium voluptatum deleniti atque corrupti quos dolores et quas molestias excepturi sint occaecati cupiditate non provident, similique sunt in culpa qui officia deserunt mollitia animi, id est laborum et dolorum fuga. Et harum quidem rerum facilis est et expedita distinctio. Nam libero tempore, cum soluta nobis est eligendi optio cumque nihil impedit quo minus id quod maxime placeat facere possimus, omnis voluptas assumenda est, omnis dolor repellendus. Temporibus autem quibusdam et aut officiis debitis aut rerum necessitatibus saepe eveniet ut et voluptates repudiandae sint et molestiae non recusandae. Itaque earum rerum hic tenetur a sapiente delectus, ut aut reiciendis voluptatibus maiores alias consequatur aut perferendis doloribus asperiores repellat.",
        XDialogIcon::Information,
    ).unwrap();

    let _ =
        xdialog::show_message_ok_cancel("Title", "Main instruction", "Hello World", XDialogIcon::Information)
            .unwrap();

    let _ = xdialog::show_message_ok_cancel(
        "Title",
        "Main instruction",
        "Hello World this is a short body but it's still long enough that the text should wrap at least once.",
        XDialogIcon::Information,
    ).unwrap();

    let _ = xdialog::show_message_ok_cancel(
        "Title",
        "Main instruction",
        "On the other hand, we denounce with righteous indignation and dislike men who are so beguiled and demoralized by the charms of pleasure of the moment, so blinded by desire, that they cannot foresee the pain and trouble that are bound to ensue; and equal blame belongs to those who fail in their duty through weakness of will, which is the same as saying through shrinking from toil and pain. These cases are perfectly simple and easy to distinguish. In a free hour, when our power of choice is untrammelled and when nothing prevents our being able to do what we like best, every pleasure is to be welcomed and every pain avoided. But in certain circumstances and owing to the claims of duty or the obligations of business it will frequently occur that pleasures have to be repudiated and annoyances accepted. The wise man therefore always holds in these matters to this principle of selection: he rejects pleasures to secure other greater pleasures, or else he endures pains to avoid worse pains. On the other hand, we denounce with righteous indignation and dislike men who are so beguiled and demoralized by the charms of pleasure of the moment, so blinded by desire, that they cannot foresee the pain and trouble that are bound to ensue; and equal blame belongs to those who fail in their duty through weakness of will, which is the same as saying through shrinking from toil and pain. These cases are perfectly simple and easy to distinguish. In a free hour, when our power of choice is untrammelled and when nothing prevents our being able to do what we like best, every pleasure is to be welcomed and every pain avoided. But in certain circumstances and owing to the claims of duty or the obligations of business it will frequently occur that pleasures have to be repudiated and annoyances accepted. The wise man therefore always holds in these matters to this principle of selection: he rejects pleasures to secure other greater pleasures, or else he endures pains to avoid worse pains.",
        XDialogIcon::Information,
    ).unwrap();

    let _ = xdialog::show_message_ok_cancel(
        "Title",
        "Main instruction",
        "Hello World\n\nThis is a multi-line dialog that has some element of text wrapping but also \n\nSome\nElement\nOf\nNewline\nBehavior\nTo make things interesting.",
        XDialogIcon::Information,
    ).unwrap();

    return 0;
}
