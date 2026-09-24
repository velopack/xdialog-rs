//! Fluent theme gallery variants: the WinUI 3 reference variants, light and dark, with the reference machine's **purple** accent palette
//! (plus a few default-blue variants for review, no reference).
//!
//! Strings come from the WinUI reference generator (not in this repository). xdialog shows
//! buttons in REVERSED API order (affirmative first, the last API index is the
//! default/accent), so each WinUI display order `[A, B, C]` is the API list `[C, B, A]`.
//! References:
//! - `winui/standalone/sw_solid_<v>_<theme>_client.png` (1:1 client captures);
//! - ContentDialog captures (`states/*_dialog.png`, `contentdialog/cd_progress_*`,
//!   `anim/progressbar_indeterminate_dialog_*`) are the same layout plus a 1 px border: cropped by
//!   1 px on each side;
//! - `anim/transition_*` are button crops: checked through per-frame fill probes from their
//!   `timing.csv` (±1 frame, ~20 ms composition cadence) instead of images.
//!
//! Not reproducible offscreen: `pressed -> pointerover` (the release would click and close the
//! dialog; WinUI drove it with `GoToState`), dialog open/close animations (the first show has none).
//!
//! Tune probes / add variants freely.

use super::*;

/// Reference machine accent palette `[L3, L2, L1, A, D1, D2, D3]`.
pub const PURPLE: [[u8; 3]; 7] = [[0xF0, 0xC0, 0xF4], [0xDB, 0x9E, 0xE5], [0xB7, 0x63, 0xCB], [0xA9, 0x4D, 0xC1], [0x8E, 0x3A, 0xA7], [0x69, 0x27, 0x82], [0x40, 0x0E, 0x59]];

const LONG: &str = "Velopack is about to apply an update to this application. The update package has been downloaded and verified, and the application will restart automatically once the installation has finished. Any unsaved work in open windows may be lost if you continue. If you choose to postpone the update, it will be applied the next time the application is started. This paragraph is intentionally long so that the text wraps across several lines at the maximum dialog width of 548 pixels.";

fn purple(dark: bool) -> TestAppearance {
    TestAppearance { dark, accent_palette: Some(PURPLE), accent: None }
}

/// Top-area and footer colours sampled against the reference.
fn surface_probes() -> Vec<Probe> {
    vec![Probe::vs_ref("top area", Anchor::Client, (3.0, 3.0), 2), Probe::vs_ref("footer", Anchor::Client, (3.0, -3.0), 2)]
}

fn standalone(name: &str, th: &str) -> RefSpec {
    RefSpec::new(format!("winui/standalone/sw_solid_{name}_{th}_client.png"), Crop::Full)
}

/// A ContentDialog capture of a `w × h` standalone layout (1 px border cropped).
fn dialog_ref(path: String, w: u32, h: u32) -> RefSpec {
    RefSpec::new(path, Crop::Rect([1, 1, w, h]))
}

fn yesno() -> XDialogOptions {
    opts("Save changes?", "Save changes?", "Do you want to save changes to the document before closing?", XDialogIcon::None, &["No", "Yes"])
}

/// Hide the open-time keyboard focus visual with a pointer click on empty space (WinUI states
/// were captured without a focus visual), then park the pointer outside.
fn pointer_mode(v: Variant) -> Variant {
    v.at(0.5, Action::MoveTo(3.0, 3.0)).at(0.5, Action::Event(HostEvent::MouseButton { button: xdialog::__test::MouseButton::Primary, pressed: true })).at(0.5, Action::Release).at(0.5, Action::Leave)
}

/// `timing.csv` rows `(t_ms, grey)` of the standard-button transitions, by theme.
fn standard_timing(name: &str, dark: bool) -> &'static [(f64, u8)] {
    match (name, dark) {
        ("normal_to_pointerover", false) => &[(-2.19, 251), (37.86, 250), (57.88, 249), (77.9, 248), (97.92, 246), (117.94, 246)],
        ("normal_to_pointerover", true) => &[(-2.29, 45), (37.75, 46), (57.77, 47), (77.79, 49), (97.81, 49), (117.83, 50)],
        ("pointerover_to_normal", false) => &[(-1.65, 246), (38.39, 246), (58.41, 248), (78.43, 248), (98.45, 250), (118.47, 251)],
        ("pointerover_to_normal", true) => &[(-0.41, 50), (39.63, 49), (59.65, 49), (79.67, 47), (99.69, 46), (119.71, 45)],
        ("pointerover_to_pressed", false) => &[(-1.0, 246), (39.04, 246), (59.06, 245), (79.08, 246), (99.1, 245), (119.11, 245)],
        ("pointerover_to_pressed", true) => &[(-1.77, 50), (38.26, 49), (58.28, 46), (78.3, 43), (98.32, 41), (118.34, 39)],
        _ => &[],
    }
}

/// `timing.csv` rows `(t_ms, rgb)` of the accent-button transitions (instant in WinUI).
fn accent_timing(name: &str, dark: bool) -> [(f64, [u8; 3]); 2] {
    match (name, dark) {
        ("normal_to_pointerover", false) => [(-2.68, [142, 58, 167]), (37.36, [152, 77, 175])],
        ("normal_to_pointerover", true) => [(-2.01, [219, 158, 229]), (38.03, [200, 145, 209])],
        ("pointerover_to_normal", false) => [(-1.61, [152, 77, 175]), (38.43, [142, 58, 167])],
        ("pointerover_to_normal", true) => [(-2.04, [200, 145, 209]), (38.01, [219, 158, 229])],
        ("pointerover_to_pressed", false) => [(-1.89, [152, 77, 175]), (38.15, [162, 95, 182])],
        _ /* pointerover_to_pressed dark */ => [(-2.06, [200, 145, 209]), (37.98, [181, 132, 189])],
    }
}

/// All variants for the fluent theme.
pub fn variants() -> Vec<Variant> {
    let mut v = Vec::new();
    for dark in [false, true] {
        let th = theme_word(dark);
        let look = purple(dark);

        // ---- standalone "solid" dialogs (sw_solid_*) ----
        let sw = |name: &str, o: XDialogOptions| {
            pointer_mode(Variant::message(format!("{name}_{th}"), o, look.clone())).capture(1.0, "").reference(standalone(name, th)).probes(surface_probes())
        };
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

        // ---- ContentDialog captures (1 px border cropped) ----
        let info_o = opts("Information", "Information", "The operation completed successfully.", XDialogIcon::None, &["OK"]);
        let long_title_o = opts("Long title", "This is a considerably longer dialog title that needs to wrap onto a second line", "Short body.", XDialogIcon::None, &["OK"]);
        let warning_o = opts("Warning", "Warning", "The application is running from a temporary folder. Updates may not work correctly.", XDialogIcon::Warning, &["Cancel", "Continue"]);
        let cd = |name: &str, o: XDialogOptions, w: u32, h: u32| {
            pointer_mode(Variant::message(format!("cd_{name}_{th}"), o, look.clone())).capture(1.0, "")
                                                                                    .reference(dialog_ref(format!("winui/contentdialog/cd_{name}_{th}.png"), w, h))
                                                                                    .probes(surface_probes())
        };
        v.push(cd("info_accentclose", info_o.clone(), 318, 187));
        v.push(cd("long_title", long_title_o.clone(), 533, 214).golden());
        // Opened without pointer input: the default button shows the keyboard focus visual
        // (`contentdialog/kbfocus/`).
        let kb = |name: &str, o: XDialogOptions, w: u32, h: u32| {
            Variant::message(format!("kbfocus_{name}_{th}"), o, look.clone()).capture(1.0, "")
                                                                             .reference(dialog_ref(format!("winui/contentdialog/kbfocus/cd_{name}_{th}_kbfocus.png"), w, h))
                                                                             .probes(surface_probes())
        };
        v.push(kb("yesno", yesno(), 435, 187).golden());
        v.push(kb("info_accentclose", info_o, 318, 187));
        v.push(kb("icon_warning", warning_o, 517, 206));
        v.push(Variant::progress(format!("kbfocus_progress_050_{th}"), opts("Installing update", "Installing update", "Downloading package... 50%", XDialogIcon::None, &["Cancel"]), look.clone())
               .at(0.2, Action::Progress(TestProgress::Value(0.5)))
               .capture(1.0, "")
               .reference(dialog_ref(format!("winui/contentdialog/kbfocus/cd_progress_050_{th}_kbfocus.png"), 348, 202)));

        // ---- progress ----
        let prog = |name: &str, body: &str| pointer_mode(Variant::progress(format!("{name}_{th}"), opts("Installing update", "Installing update", body, XDialogIcon::None, &["Cancel"]), look.clone()));
        v.push(prog("progress_050", "Downloading package... 50%").at(0.2, Action::Progress(TestProgress::Value(0.5)))
                                                                  .capture(1.0, "")
                                                                  .reference(standalone("progress_050", th))
                                                                  .probes(surface_probes()));
        v.push(prog("progress_000", "Downloading package... 0%").capture(1.0, "")
                                                                 .reference(dialog_ref(format!("winui/contentdialog/cd_progress_000_{th}.png"), 348, 202))
                                                                 .golden());
        v.push(prog("progress_100", "Downloading package... 100%").at(0.2, Action::Progress(TestProgress::Value(1.0)))
                                                                   .capture(1.0, "")
                                                                   .reference(dialog_ref(format!("winui/contentdialog/cd_progress_100_{th}.png"), 348, 202)));
        v.push(prog("progress_indeterminate", "Preparing...").at(0.2, Action::Progress(TestProgress::Indeterminate))
                                                              .capture(1.0, "")
                                                              .reference(standalone("progress_indeterminate", th)));
        // 16 frames at 50 ms. The capture sequence starts 1.5 s into the 2 s loop (its t = 0.55 s
        // frame shows bar 1 entering), so the burst starts at `since + 1.5` (dark: one 50 ms frame
        // earlier, its sequence starts at 1.45 s).
        let mut frames = Vec::new();
        for k in 0..16 {
            frames.push((format!("_f{k:02}"), format!("winui/anim/progressbar_indeterminate_dialog_{th}/f{k:02}_t{:04}ms.png", k * 50)));
        }
        let mut r = dialog_ref(String::new(), 348, 202);
        r.frames = frames;
        v.push(prog("progress_indeterminate_anim", "Preparing...").at(1.0, Action::Progress(TestProgress::Indeterminate)).burst(if dark { 2.45 } else { 2.5 }, 16, 0.05).reference(r));

        // ---- button states in the yesno dialog (states/cd_btn_*) ----
        // API: 0 = "No" (standard, CloseButton), 1 = "Yes" (accent, PrimaryButton/default).
        let (std_b, acc_b) = (0usize, 1usize);
        // `disabled` / `focused_pointerover` of a single kind exist only as button crops
        // (`*_button.png`, `*_button_x4.png`): no dialog reference; review by eye.
        let state = |kind: &str, state: &str| {
            let v = Variant::message(format!("btn_{kind}_{state}_{th}"), yesno(), look.clone());
            if kind != "both" && (state == "disabled" || state == "focused_pointerover") {
                return v;
            }
            v.reference(dialog_ref(format!("winui/states/cd_btn_{kind}_{state}_{th}_dialog.png"), 435, 187))
             .probes(surface_probes())
             .probe(Probe::vs_ref("button fill", Anchor::ButtonMid(if kind == "accent" { acc_b } else { std_b }), (6.0, 0.0), 3))
        };
        // The `standard` captures kept the default button's keyboard focus visual (WinUI drove the
        // states with GoToState, focus untouched), the `accent` ones hid it: open without / with a
        // pointer click. `standard pressed` can't match: a real press moves focus to "No" and the
        // accent follows focus (the reference keeps "Yes" accent + focused); probe the fill only.
        for (kind, b) in [("standard", std_b), ("accent", acc_b)] {
            let mode = |v: Variant| if kind == "standard" { v } else { pointer_mode(v) };
            v.push(mode(state(kind, "normal")).capture(1.0, ""));
            v.push(mode(state(kind, "pointerover")).at(1.0, Action::HoverButton(b)).capture(1.5, ""));
            v.push(mode(state(kind, "pressed")).at(1.0, Action::PressButton(b)).capture(1.5, ""));
            v.push(mode(state(kind, "disabled")).at(1.0, Action::Disable(b, true)).capture(1.5, ""));
        }
        v.push(pointer_mode(state("both", "disabled")).at(1.0, Action::Disable(0, true)).at(1.0, Action::Disable(1, true)).capture(1.5, ""));
        // Keyboard focus: the default (accent) button has it on open; Tab moves it to "No"
        // (display order Yes, No) and the accent style follows focus.
        v.push(state("accent", "focused").capture(1.0, ""));
        v.push(state("accent", "focused_pointerover").at(1.0, Action::HoverButton(acc_b)).capture(1.5, ""));
        v.push(state("standard", "focused").at(1.0, Action::Key(Key::Tab)).capture(1.5, ""));
        v.push(state("standard", "focused_pointerover").at(1.0, Action::Key(Key::Tab)).at(1.0, Action::HoverButton(std_b)).capture(1.5, ""));

        // ---- transitions (anim/transition_*; timing.csv fill probes) ----
        let t0 = 2.0;
        for name in ["normal_to_pointerover", "pointerover_to_normal", "pointerover_to_pressed"] {
            let setup = |v: Variant, b: usize| {
                let v = pointer_mode(v);
                match name {
                    "normal_to_pointerover" => v.at(t0, Action::HoverButton(b)),
                    "pointerover_to_normal" => v.at(1.0, Action::HoverButton(b)).at(t0, Action::MoveTo(3.0, 3.0)),
                    _ => v.at(1.0, Action::HoverButton(b)).at(t0, Action::Event(HostEvent::MouseButton { button: xdialog::__test::MouseButton::Primary, pressed: true })),
                }
            };
            let mut sv = setup(Variant::message(format!("transition_standard_{name}_{th}"), yesno(), look.clone()), std_b);
            for (k, (ms, grey)) in standard_timing(name, dark).iter().enumerate() {
                let sfx = format!("_f{k:02}");
                sv = sv.capture(t0 + ms / 1000.0, &sfx).probe(Probe::new("bg", Anchor::ButtonMid(std_b), (6.0, 0.0), [*grey; 3], 1).only(sfx));
            }
            v.push(sv);
            let mut av = setup(Variant::message(format!("transition_accent_{name}_{th}"), yesno(), look.clone()), acc_b);
            for (k, (ms, rgb)) in accent_timing(name, dark).iter().enumerate() {
                let sfx = format!("_f{k:02}");
                av = av.capture(t0 + ms / 1000.0, &sfx).probe(Probe::new("bg", Anchor::ButtonMid(acc_b), (6.0, 0.0), *rgb, 2).only(sfx));
            }
            v.push(av);
        }

        // ---- focus/accent interaction (review only, no reference) ----
        // A pointer press on "No" moves focus and the accent in the SAME frame as the press look.
        let std_rest = if dark { [0x2D; 3] } else { [0xFB; 3] };
        v.push(Variant::message(format!("interaction_press_no_{th}"), yesno(), look.clone()).at(1.0, Action::PressButton(std_b))
                                                                                          .capture(1.0, "_t0")
                                                                                          .capture(1.017, "_t17ms")
                                                                                          .probe(Probe::new("yes fill", Anchor::ButtonMid(acc_b), (6.0, 0.0), std_rest, 2)));
        // Default button disabled before open: focus (with the keyboard ring) goes to "No".
        v.push(Variant::message(format!("edge_default_disabled_{th}"), yesno(), look.clone()).at(0.0, Action::Disable(acc_b, true)).capture(1.0, ""));

        // ---- default blue accent (review only, no reference) ----
        v.push(Variant::message(format!("blue_yesno_{th}"), yesno(), super::look(dark)).capture(1.0, ""));
        v.push(Variant::message(format!("blue_icon_warning_{th}"),
                                opts("Warning", "Warning", "The application is running from a temporary folder. Updates may not work correctly.", XDialogIcon::Warning, &["Cancel", "Continue"]),
                                super::look(dark)).capture(1.0, ""));
    }
    // Edge cases (review only, no reference): scrolling body, missing parts, many buttons.
    let huge: String = (1..=80).map(|i| format!("Line {i} of a very long message that has to scroll.")).collect::<Vec<_>>().join("
");
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
