//! Fluent theme gallery variants, light and dark, with a **purple** accent palette (plus a few
//! default-blue variants).
//!
//! xdialog shows buttons in REVERSED API order (affirmative first, the last API index is the
//! default/accent), so a WinUI display order `[A, B, C]` is the API list `[C, B, A]`.
//! Stills are captured at `t = 1.0` (suffix ""); the 83 ms button transitions at the trigger
//! (`_t0`), +40 ms and +120 ms.

use super::*;

/// Accent palette `[L3, L2, L1, A, D1, D2, D3]`.
pub const PURPLE: [[u8; 3]; 7] = [[0xF0, 0xC0, 0xF4], [0xDB, 0x9E, 0xE5], [0xB7, 0x63, 0xCB], [0xA9, 0x4D, 0xC1], [0x8E, 0x3A, 0xA7], [0x69, 0x27, 0x82], [0x40, 0x0E, 0x59]];

const LONG: &str = "Velopack is about to apply an update to this application. The update package has been downloaded and verified, and the application will restart automatically once the installation has finished. Any unsaved work in open windows may be lost if you continue. If you choose to postpone the update, it will be applied the next time the application is started. This paragraph is intentionally long so that the text wraps across several lines at the maximum dialog width of 548 pixels.";

fn purple(dark: bool) -> TestAppearance {
    TestAppearance { dark, accent_palette: Some(PURPLE), accent: None }
}

fn yesno() -> XDialogOptions {
    opts("Save changes?", "Save changes?", "Do you want to save changes to the document before closing?", XDialogIcon::None, &["No", "Yes"])
}

/// Hide the open-time keyboard focus visual with a pointer click on empty space, then park the
/// pointer outside.
fn pointer_mode(v: Variant) -> Variant {
    v.at(0.5, Action::MoveTo(3.0, 3.0)).at(0.5, Action::Press).at(0.5, Action::Release).at(0.5, Action::Leave)
}

/// All variants for the fluent theme.
pub fn variants() -> Vec<Variant> {
    let mut v = Vec::new();
    for dark in [false, true] {
        let th = theme_word(dark);
        let look = purple(dark);

        // ---- standalone dialogs (pointer mode: no focus visual) ----
        let sw = |name: &str, o: XDialogOptions| pointer_mode(Variant::message(format!("{name}_{th}"), o, look.clone())).capture(1.0, "");
        v.push(sw("info", opts("Information", "Information", "The operation completed successfully.", XDialogIcon::None, &["OK"])).golden());
        v.push(sw("yesno", yesno()).golden());
        v.push(sw("retrycancel",
                  opts("Download failed", "Download failed", "The update could not be downloaded. Check your network connection and try again.", XDialogIcon::None, &["Cancel", "Retry"])));
        v.push(sw("three_buttons", opts("Save your work?", "Save your work?", "Your changes will be lost if you don't save them.", XDialogIcon::None, &["Cancel", "Don't save", "Save"])));
        v.push(sw("icon_info", opts("Information", "Information", "A new version of this application is available.", XDialogIcon::Information, &["OK"])).golden());
        v.push(sw("icon_warning",
                  opts("Warning", "Warning", "The application is running from a temporary folder. Updates may not work correctly.", XDialogIcon::Warning, &["Cancel", "Continue"])));
        v.push(sw("icon_error", opts("Error", "Error", "The installation failed. The package signature could not be verified.", XDialogIcon::Error, &["Close"])));
        v.push(sw("long_text", opts("Update ready to install", "Update ready to install", LONG, XDialogIcon::None, &["Later", "Restart now"])));

        let info_o = opts("Information", "Information", "The operation completed successfully.", XDialogIcon::None, &["OK"]);
        let long_title_o = opts("Long title", "This is a considerably longer dialog title that needs to wrap onto a second line", "Short body.", XDialogIcon::None, &["OK"]);
        let warning_o = opts("Warning", "Warning", "The application is running from a temporary folder. Updates may not work correctly.", XDialogIcon::Warning, &["Cancel", "Continue"]);
        v.push(sw("cd_info_accentclose", info_o.clone()));
        v.push(sw("cd_long_title", long_title_o).golden());
        // Opened without pointer input: the default button shows the keyboard focus visual.
        let kb = |name: &str, o: XDialogOptions| Variant::message(format!("kbfocus_{name}_{th}"), o, look.clone()).capture(1.0, "");
        v.push(kb("yesno", yesno()).golden());
        v.push(kb("info_accentclose", info_o));
        v.push(kb("icon_warning", warning_o));
        v.push(Variant::progress(format!("kbfocus_progress_050_{th}"), opts("Installing update", "Installing update", "Downloading package... 50%", XDialogIcon::None, &["Cancel"]), look.clone())
               .at(0.2, Action::Progress(TestProgress::Value(0.5)))
               .capture(1.0, ""));

        // ---- progress ----
        let prog = |name: &str, body: &str| pointer_mode(Variant::progress(format!("{name}_{th}"), opts("Installing update", "Installing update", body, XDialogIcon::None, &["Cancel"]), look.clone()));
        v.push(prog("progress_050", "Downloading package... 50%").at(0.2, Action::Progress(TestProgress::Value(0.5))).capture(1.0, ""));
        v.push(prog("progress_000", "Downloading package... 0%").capture(1.0, "").golden());
        v.push(prog("progress_100", "Downloading package... 100%").at(0.2, Action::Progress(TestProgress::Value(1.0))).capture(1.0, ""));
        v.push(prog("progress_indeterminate", "Preparing...").at(0.2, Action::Progress(TestProgress::Indeterminate)).capture(1.0, ""));
        v.push(prog("progress_indeterminate_anim", "Preparing...").at(1.0, Action::Progress(TestProgress::Indeterminate))
                                                                   .capture(1.5, "_t500ms")
                                                                   .capture(2.0, "_t1000ms")
                                                                   .capture(2.5, "_t1500ms"));

        // ---- button states in the yesno dialog ----
        // API: 0 = "No" (standard), 1 = "Yes" (accent, default). The `standard` states keep the
        // default button's keyboard focus visual, the `accent` ones hide it (pointer mode).
        let (std_b, acc_b) = (0usize, 1usize);
        let state = |kind: &str, state: &str| Variant::message(format!("btn_{kind}_{state}_{th}"), yesno(), look.clone());
        for (kind, b) in [("standard", std_b), ("accent", acc_b)] {
            let mode = |v: Variant| if kind == "standard" { v } else { pointer_mode(v) };
            v.push(mode(state(kind, "normal")).capture(1.0, ""));
            v.push(mode(state(kind, "pointerover")).at(1.0, Action::HoverButton(b)).capture(1.5, ""));
            v.push(mode(state(kind, "pressed")).at(1.0, Action::PressButton(b)).capture(1.5, ""));
        }
        // Keyboard focus: the default (accent) button has it on open; Tab moves it to "No" and the
        // accent style follows focus.
        v.push(state("accent", "focused").capture(1.0, ""));
        v.push(state("accent", "focused_pointerover").at(1.0, Action::HoverButton(acc_b)).capture(1.5, ""));
        v.push(state("standard", "focused").at(1.0, Action::Key(Key::Tab)).capture(1.5, ""));
        v.push(state("standard", "focused_pointerover").at(1.0, Action::Key(Key::Tab)).at(1.0, Action::HoverButton(std_b)).capture(1.5, ""));

        // ---- transitions (83 ms) ----
        for name in ["normal_to_pointerover", "pointerover_to_normal", "pointerover_to_pressed"] {
            for (kind, b) in [("standard", std_b), ("accent", acc_b)] {
                let t = pointer_mode(Variant::message(format!("transition_{kind}_{name}_{th}"), yesno(), look.clone()));
                let t = match name {
                    "normal_to_pointerover" => t.at(1.0, Action::HoverButton(b)),
                    "pointerover_to_normal" => t.at(0.8, Action::HoverButton(b)).at(1.0, Action::MoveTo(3.0, 3.0)),
                    _ => t.at(0.8, Action::HoverButton(b)).at(1.0, Action::Press),
                };
                v.push(t.capture(1.0, "_t0").capture(1.04, "_t40ms").capture(1.12, "_t120ms"));
            }
        }

        // A pointer press on "No" moves focus and the accent in the SAME frame as the press look.
        v.push(Variant::message(format!("interaction_press_no_{th}"), yesno(), look.clone()).at(1.0, Action::PressButton(std_b))
                                                                                          .capture(1.0, "_t0")
                                                                                          .capture(1.017, "_t17ms"));

        // ---- default blue accent ----
        v.push(Variant::message(format!("blue_yesno_{th}"), yesno(), super::look(dark)).capture(1.0, ""));
        v.push(Variant::message(format!("blue_icon_warning_{th}"),
                                opts("Warning", "Warning", "The application is running from a temporary folder. Updates may not work correctly.", XDialogIcon::Warning, &["Cancel", "Continue"]),
                                super::look(dark)).capture(1.0, ""));
    }
    // Edge cases: scrolling body, missing parts, many buttons.
    let huge: String = (1..=80).map(|i| format!("Line {i} of a very long message that has to scroll.")).collect::<Vec<_>>().join("\n");
    for dark in [false, true] {
        let th = theme_word(dark);
        let look = purple(dark);
        v.push(Variant::message(format!("edge_scroll_{th}"), opts("Scroll", "A very long message", &huge, XDialogIcon::Information, &["Cancel", "OK"]), look.clone()).capture(1.0, "")
                                                                                                                                                          .at(1.5, Action::Key(Key::PageDown))
                                                                                                                                                          .capture(1.6, "_pagedown")
                                                                                                                                                          .golden());
        v.push(Variant::message(format!("edge_no_heading_{th}"), opts("x", "", "Only a body, no heading.", XDialogIcon::None, &["OK"]), look.clone()).capture(1.0, ""));
        v.push(Variant::message(format!("edge_no_buttons_{th}"), opts("x", "No buttons", "Press Escape to close.", XDialogIcon::Warning, &[]), look.clone()).capture(1.0, ""));
        v.push(Variant::message(format!("edge_icon_only_{th}"), opts("x", "Heading and icon", "", XDialogIcon::Error, &["OK"]), look.clone()).capture(1.0, ""));
        v.push(Variant::message(format!("edge_five_buttons_{th}"), opts("x", "Five buttons", "More buttons than ContentDialog supports.", XDialogIcon::None, &["One", "Two", "Three", "Four", "Five"]),
                                look.clone()).capture(1.0, ""));
        v.push(Variant::message(format!("edge_long_buttons_{th}"),
                                opts("x", "Long labels", "Labels wider than their column are elided.", XDialogIcon::None,
                                     &["Don't save changes now", "Cancel this operation", "Save everything to disk", "Keep editing the document"]),
                                look.clone()).capture(1.0, ""));
        v.push(Variant::message(format!("edge_rtl_mixed_{th}"),
                                opts("x", "مرحبا بالعالم", "This is an English paragraph.\nשלום עולם, זהו טקסט בעברית.", XDialogIcon::Information, &["إلغاء", "موافق"]),
                                look.clone()).capture(1.0, ""));
        v.push(Variant::progress(format!("edge_progress_icon_{th}"), opts("x", "Installing", "Downloading...", XDialogIcon::Information, &["Cancel"]), look.clone())
               .at(0.2, Action::Progress(TestProgress::Value(0.3)))
               .at(0.5, Action::Progress(TestProgress::Value(0.8)))
               .capture(0.5, "_t0")
               .capture(0.6, "_t100ms")
               .capture(1.0, "")
               .at(1.2, Action::SetText("A much longer status text that now wraps onto a second line because it is long enough to do so.".into()))
               .capture(1.5, "_settext"));
    }
    // HiDPI smoke.
    v.push(Variant::message("hidpi2_yesno_light", yesno(), purple(false)).ppp(2.0).capture(1.0, ""));
    v.push(Variant::message("hidpi15_yesno_dark", yesno(), purple(true)).ppp(1.5).capture(1.0, ""));
    // Fractional scales: the focus ring thicknesses land on whole physical pixels.
    v.push(Variant::message("hidpi125_kbfocus_yesno_light", yesno(), purple(false)).ppp(1.25).capture(1.0, ""));
    v.push(Variant::message("hidpi15_kbfocus_yesno_light", yesno(), purple(false)).ppp(1.5).capture(1.0, ""));
    v
}
