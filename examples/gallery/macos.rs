//! macOS theme gallery variants, light and dark, with the default (blue) accent plus a few
//! purple ones.
//!
//! Buttons are in API order left to right (the last API index is the default, blue, button);
//! three or more, or labels too long for half the row, stack with the default on top. Stills are
//! captured at `t = 1.0` (suffix "").

use super::*;

const LONG: &str = "Velopack is about to apply an update to this application. The update package has been downloaded and verified, and \
                    the application will restart automatically once the installation has finished. Any unsaved work in open windows may \
                    be lost if you continue.";

fn purple(dark: bool) -> TestAppearance {
    TestAppearance { dark, accent_palette: None, accent: Some([0xA5, 0x50, 0xA7]) }
}

/// A single-capture message variant (`t = 1.0`).
fn still(name: &str, dark: bool, o: XDialogOptions) -> Variant {
    Variant::message(format!("{name}_{}", theme_word(dark)), o, look(dark)).capture(1.0, "")
}

fn quit() -> XDialogOptions {
    opts("Karabiner-Elements",
         "Are you sure you want to quit Karabiner-Elements?",
         "The changed key will be restored after Karabiner-Elements is quit.",
         XDialogIcon::Information,
         &["Cancel", "Quit"])
}

/// All variants for the macOS theme.
pub fn variants() -> Vec<Variant> {
    let mut v = Vec::new();
    for dark in [false, true] {
        let th = theme_word(dark);

        // ---- static dialogs ----
        v.push(still("quit", dark, quit()).golden());
        v.push(still("not_opened",
                     dark,
                     opts("x",
                          "“Example App” Not Opened",
                          "Apple could not verify “Example App” is free of malware that may harm your Mac or compromise your privacy.",
                          XDialogIcon::Warning,
                          &["Done", "Move to Trash"])).golden());
        v.push(still("error_ok",
                     dark,
                     opts("x", "The operation couldn’t be completed.", "The disk is not mounted.", XDialogIcon::Error, &["OK"])));
        v.push(still("noicon_okcancel",
                     dark,
                     opts("x", "Close the application?", "Any unsaved changes will be lost.", XDialogIcon::None, &["Cancel", "OK"])));
        v.push(still("three_buttons",
                     dark,
                     opts("x",
                          "Do you want to save the changes made to the document “Untitled”?",
                          "Your changes will be lost if you don’t save them.",
                          XDialogIcon::Warning,
                          &["Cancel", "Don’t Save", "Save…"])).golden());
        v.push(still("long_labels",
                     dark,
                     opts("x",
                          "Update available",
                          "A new version is ready to install.",
                          XDialogIcon::Information,
                          &["Remind Me Later", "Install and Relaunch"])));
        v.push(still("long_text", dark, opts("x", "Update ready to install", LONG, XDialogIcon::Information, &["Later", "Restart Now"])));
        v.push(still("title_only_body",
                     dark,
                     opts("Window Title",
                          "",
                          "Only a body: the window title stands in as the alert's title.",
                          XDialogIcon::None,
                          &["OK"])));
        v.push(still("heading_only", dark, opts("x", "Heading and icon only", "", XDialogIcon::Error, &["OK"])));
        v.push(still("no_buttons", dark, opts("x", "No buttons", "Press Escape to close.", XDialogIcon::Warning, &[])));
        v.push(still("rtl_mixed",
                     dark,
                     opts("x",
                          "مرحبا بالعالم",
                          "This is an English paragraph.\nשלום עולם, זהו טקסט בעברית.",
                          XDialogIcon::Information,
                          &["إلغاء", "موافق"])));

        // ---- keyboard and pointer ----
        // Tab shows the focus ring (moves to "Cancel"); the default stays blue.
        v.push(Variant::message(format!("kbfocus_tab_{th}"), quit(), look(dark)).at(1.0, Action::Key(Key::Tab)).capture(1.5, "").golden());
        v.push(Variant::message(format!("hover_default_{th}"), quit(), look(dark)).at(1.0, Action::HoverButton(1)).capture(1.5, ""));
        v.push(Variant::message(format!("hover_standard_{th}"), quit(), look(dark)).at(1.0, Action::HoverButton(0)).capture(1.5, ""));
        v.push(Variant::message(format!("pressed_default_{th}"), quit(), look(dark)).at(1.0, Action::PressButton(1)).capture(1.5, ""));
        v.push(Variant::message(format!("pressed_standard_{th}"), quit(), look(dark)).at(1.0, Action::PressButton(0)).capture(1.5, ""));

        // ---- progress ----
        let prog = |name: &str, body: &str| {
            Variant::progress(format!("{name}_{th}"),
                              opts("x", "Installing update", body, XDialogIcon::Information, &["Cancel"]),
                              look(dark))
        };
        v.push(prog("progress_000", "Downloading package… 0%").capture(1.0, "").golden());
        v.push(prog("progress_050", "Downloading package… 50%").at(0.2, Action::Progress(TestProgress::Value(0.5))).capture(1.0, ""));
        v.push(prog("progress_indeterminate", "Preparing…").at(0.2, Action::Progress(TestProgress::Indeterminate))
                                                           .capture(1.0, "")
                                                           .capture(1.5, "_t500ms"));
        v.push(Variant::progress(format!("progress_nobuttons_{th}"), opts("x", "Copying files", "", XDialogIcon::None, &[]), look(dark))
               .at(0.2, Action::Progress(TestProgress::Value(0.3)))
               .capture(1.0, ""));

        // ---- accent ----
        v.push(Variant::message(format!("purple_quit_{th}"), quit(), purple(dark)).capture(1.0, ""));
    }
    let huge: String = (1..=80).map(|i| format!("Line {i} of a very long message that has to scroll.")).collect::<Vec<_>>().join("\n");
    for dark in [false, true] {
        let th = theme_word(dark);
        v.push(Variant::message(format!("edge_scroll_{th}"),
                                opts("x", "A very long message", &huge, XDialogIcon::Information, &["Cancel", "OK"]),
                                look(dark)).capture(1.0, "")
                                           .at(1.5, Action::Key(Key::PageDown))
                                           .capture(1.6, "_pagedown"));
    }
    v.push(Variant::message("hidpi2_quit_light", quit(), look(false)).ppp(2.0).capture(1.0, ""));
    v.push(Variant::message("hidpi2_not_opened_dark",
                            opts("x",
                                 "“Example App” Not Opened",
                                 "Apple could not verify “Example App” is free of malware that may harm your Mac or compromise your \
                                  privacy.",
                                 XDialogIcon::Warning,
                                 &["Done", "Move to Trash"]),
                            look(true)).ppp(2.0)
                                       .capture(1.0, ""));
    v
}
