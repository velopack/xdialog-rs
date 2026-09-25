use xdialog::{show_progress, XDialogIcon};

/// Optional argument: `light` or `dark` (default: follow the system).
fn main() {
    let theme = match std::env::args().nth(1).as_deref() {
        Some("light") => xdialog::XDialogTheme::Light,
        Some("dark") => xdialog::XDialogTheme::Dark,
        _ => xdialog::XDialogTheme::SystemDefault,
    };
    xdialog::XDialogBuilder::new().with_theme(theme).run(run);
}

fn run() {
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
    let _ = xdialog::show_message(data.clone(), None);

    data.message = medium_text.to_string();
    let _ = xdialog::show_message(data.clone(), None);

    // Taller than the height limit: the body scrolls.
    data.message = medium_text.repeat(3);
    let _ = xdialog::show_message(data.clone(), None);

    data.message = "Hello World\n\nThis is a multi-line dialog with wrapping text and\n\nSome\nElement\nOf\nNewline\nBehavior".to_string();
    let _ = xdialog::show_message(data.clone(), None);

    data.message = small_text.to_string();
    data.main_instruction = long_instruction.to_string();
    let _ = xdialog::show_message(data.clone(), None);

    data.icon = XDialogIcon::Error;
    let _ = xdialog::show_message(data.clone(), None);

    data.message = medium_text.to_string();
    data.title = "".to_string();
    let _ = xdialog::show_message(data.clone(), None);

    // Unicode: many scripts and emoji, right-to-left and mixed text, complex emoji sequences.
    let _ = xdialog::show_message_ok_cancel(
        "🌍 Unicode Test",
        "Hello from around the world! 👋",
        "English: Hello\nGerman: Grüße\nJapanese: こんにちは\nChinese: 你好世界\nKorean: 안녕하세요\nThai: สวัสดี\nHindi: नमस्ते\nRussian: Привет мир",
        XDialogIcon::Information,
    );
    let _ = xdialog::show_message_ok_cancel(
        "🔄 Bidirectional Text",
        "مرحبا بالعالم",
        "This is an English paragraph.\nשלום עולם, זהו טקסט בעברית.\nMixed: The word شمس means sun",
        XDialogIcon::Information,
    );
    let data = xdialog::XDialogOptions {
        icon: XDialogIcon::Information,
        title: "👨‍👩‍👧‍👦 Complex Emoji".to_string(),
        main_instruction: "Family & Skin Tone Modifiers 🏽".to_string(),
        message: "Skin tones: 👋🏻👋🏼👋🏽👋🏾👋🏿\nFamilies: 👨‍👩‍👧‍👦 👩‍👩‍👦‍👦\nCompound: 🏳️‍🌈 🏴‍☠️ 🐻‍❄️\nKeycaps: 0️⃣1️⃣2️⃣🔟".to_string(),
        buttons: vec!["Looks Good! 👍".to_string(), "Broken 💔".to_string()],
    };
    let _ = xdialog::show_message(data, None);

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
}
