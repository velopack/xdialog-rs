//! Linux (skia-look) theme gallery variants, light and dark.
//!
//! Stills are captured at `t = 1.0` (suffix ""). Transitions are sampled at a few points: the
//! 150 ms fades at the trigger (`_t0`), +75 ms (`_t75ms`) and settled (+200 ms, ""); the 300 ms
//! progress animation at +150 ms (`_t150ms`) and settled (+300 ms, ""); the 3 s indeterminate
//! capsule three times across one cycle.

use super::*;

const LONG: &str = "At vero eos et accusamus et iusto odio dignissimos ducimus qui blanditiis praesentium voluptatum deleniti atque corrupti quos dolores et quas molestias excepturi sint occaecati cupiditate non provident, similique sunt in culpa qui officia deserunt mollitia animi, id est laborum et dolorum fuga. Et harum quidem rerum facilis est et expedita distinctio. Nam libero tempore, cum soluta nobis est eligendi optio cumque nihil impedit quo minus id quod maxime placeat facere possimus, omnis voluptas assumenda est, omnis dolor repellendus.";

/// A single-capture message variant (`t = 1.0`).
fn still(name: &str, dark: bool, o: XDialogOptions) -> Variant {
    Variant::message(format!("{name}_{}", theme_word(dark)), o, look(dark)).capture(1.0, "")
}

/// Captures of a 150 ms fade triggered at `t`.
fn fade(v: Variant, t: f64) -> Variant {
    v.capture(t, "_t0").capture(t + 0.075, "_t75ms").capture(t + 0.2, "")
}

/// Captures of the 300 ms progress animation triggered at `t`.
fn grow(v: Variant, t: f64) -> Variant {
    v.capture(t + 0.15, "_t150ms").capture(t + 0.3, "")
}

fn states_opts(title: &str) -> XDialogOptions {
    opts(title, "New version available", "Would you like to update to the new version now?", XDialogIcon::Warning, &["No", "Yes"])
}

fn progress_opts(title: &str, buttons: &[&str]) -> XDialogOptions {
    opts(title, "Downloading updates", "Downloading package 1 of 3...", XDialogIcon::Information, buttons)
}

/// All variants for the linux theme.
pub fn variants() -> Vec<Variant> {
    let mut v = Vec::new();
    for dark in [false, true] {
        let th = theme_word(dark);

        // ---- static dialogs ----
        v.push(still("info_ok", dark, opts("Information", "Update installed", "My App has been updated to version 2.4.1 successfully.", XDialogIcon::Information, &["OK"]))
               .golden());
        v.push(still("warning_yesno", dark, states_opts("Update Available")).golden());
        v.push(still("error_retrycancel",
                     dark,
                     opts("Error", "The update failed", "The download could not be completed. Check your network connection and try again.", XDialogIcon::Error, &["Cancel", "Retry"])));
        v.push(still("noicon_okcancel", dark, opts("Confirm", "Close the application?", "Any unsaved changes will be lost.", XDialogIcon::None, &["Cancel", "OK"])).golden());
        v.push(still("long_wrap", dark, opts("Long Text", "Main instruction", LONG, XDialogIcon::Information, &["Cancel", "OK"])));
        v.push(still("long_instruction",
                     dark,
                     opts("Long Instruction",
                          "This is v. long main instruction which will almost certainly need to wrap into several lines and I need to make sure that the dialog sizes correctly",
                          "This is a very small dialog message!",
                          XDialogIcon::Error,
                          &["OK"])));
        v.push(still("body_only", dark, opts("Body Only", "", "This is a very small dialog message!", XDialogIcon::None, &["OK"])).golden());
        v.push(still("unicode_cjk",
                     dark,
                     opts("漢字テスト — CJK Test",
                          "日本語・中文・한국어の表示テスト",
                          "Japanese Hiragana: あいうえお かきくけこ さしすせそ\nJapanese Katakana: アイウエオ カキクケコ サシスセソ\nJapanese Kanji: 東京都港区六本木ヒルズ\nChinese Simplified: 中华人民共和国万岁\nChinese Traditional: 國立故宮博物院\nKorean: 대한민국 서울특별시\nVietnamese: Xin chào thế giới\nBopomofo: ㄅㄆㄇㄈㄉㄊㄋㄌ",
                          XDialogIcon::Information,
                          &["Cancel", "OK"])));
        v.push(still("unicode_world",
                     dark,
                     opts("🌍 Unicode Test",
                          "Hello from around the world! 👋",
                          "English: Hello\nSpanish: ¡Hola!\nGerman: Grüße\nJapanese: こんにちは\nChinese: 你好世界\nKorean: 안녕하세요\nArabic: مرحبا\nHebrew: שלום\nThai: สวัสดี\nHindi: नमस्ते\nRussian: Привет мир",
                          XDialogIcon::Information,
                          &["Looks Good! 👍", "Broken 💔"])));
        v.push(still("rtl_align",
                     dark,
                     opts("QA2 rtl",
                          "مرحبا بالعالم",
                          "هذا نص عربي طويل للتحقق من الالتفاف والمحاذاة في مربع الحوار. שלום עולם, זהו טקסט בעברית.",
                          XDialogIcon::Information,
                          &["إلغاء", "موافق"])));
        v.push(still("rtl_mixed",
                     dark,
                     opts("QA2 mixed",
                          "Mixed directions",
                          "This is an English paragraph.\nשלום עולם, זהו טקסט בעברית.",
                          XDialogIcon::None,
                          &["OK"])));

        // ---- button states (API 0 = "No", 1 = "Yes", the default) ----
        let (no, yes) = (0usize, 1usize);
        let st = |name: &str| Variant::message(format!("{name}_{th}"), states_opts("Update Available (states)"), look(dark));
        v.push(st("state_idle").capture(1.0, ""));
        v.push(fade(st("hover_default").at(1.0, Action::HoverButton(yes)), 1.0));
        v.push(fade(st("hover_secondary").at(1.0, Action::HoverButton(no)), 1.0));
        v.push(fade(st("unhover").at(0.5, Action::HoverButton(no)).at(1.0, Action::MoveTo(5.0, 5.0)), 1.0));
        v.push(fade(st("pressed_secondary").at(0.5, Action::HoverButton(no)).at(1.0, Action::Press), 1.0));
        v.push(st("after_press_focus_moved").at(0.5, Action::PressButton(no)).at(1.0, Action::MoveTo(5.0, 5.0)).at(1.0, Action::Release).capture(1.5, ""));

        // Keyboard focus.
        let fo = |name: &str| Variant::message(format!("{name}_{th}"), states_opts("Update Available (focus)"), look(dark));
        v.push(fo("focus_default").capture(1.0, "").golden());
        v.push(fade(fo("focus_tab").at(1.0, Action::Key(Key::Tab)), 1.0));
        v.push(fo("focus_right_arrow").at(0.5, Action::Key(Key::Tab)).at(1.0, Action::Key(Key::ArrowRight)).capture(1.5, ""));

        // The pointer leaves the window over the hovered "No": "No" fades back to idle and the
        // focus ring on "Yes" returns.
        v.push(Variant::message(format!("leave_restores_focus_{th}"), states_opts("Update Available (leave)"), look(dark))
                   .at(1.0, Action::HoverButton(no))
                   .capture(1.3, "_hovered")
                   .at(1.5, Action::Leave)
                   .capture(2.0, ""));
        // An inactive window keeps the default button's focus ring.
        v.push(Variant::message(format!("inactive_focus_{th}"), states_opts("Update Available (inactive)"), look(dark)).at(0.5, Action::WindowFocus(false))
                                                                                                                    .capture(1.0, ""));

        // ---- progress ----
        let pr = |name: &str| Variant::progress(format!("{name}_{th}"), progress_opts("Downloading (progress)", &[]), look(dark));
        v.push(pr("progress_0").capture(1.0, "").golden());
        v.push(grow(pr("progress_50").at(1.0, Action::Progress(TestProgress::Value(0.5))), 1.0));
        v.push(grow(pr("progress_100").at(0.5, Action::Progress(TestProgress::Value(0.5))).at(1.0, Action::Progress(TestProgress::Value(1.0))), 1.0));
        v.push(pr("progress_indeterminate").at(1.0, Action::Progress(TestProgress::Indeterminate))
                                           .capture(1.5, "_t500ms")
                                           .capture(2.5, "_t1500ms")
                                           .capture(3.5, "_t2500ms"));
        v.push(pr("progress_indeterminate_longtext").at(0.5, Action::Progress(TestProgress::Indeterminate))
                                                    .at(1.0,
                                                        Action::SetText("This is some long text which should wrap and cause the window size to be \
                                                                         re-calculated. This is some long text which should wrap."
                                                                                                                                   .into()))
                                                    .capture(1.5, ""));

        // Progress with a Cancel button, then hovered.
        let pc = |name: &str| {
            Variant::progress(format!("{name}_{th}"), progress_opts("Downloading (cancel)", &["Cancel"]), look(dark)).at(0.3, Action::Progress(TestProgress::Value(0.35)))
        };
        v.push(pc("progress_cancel").capture(1.0, ""));
        v.push(pc("progress_cancel_hover").at(1.0, Action::HoverButton(0)).capture(1.3, ""));

        // No icon, no title.
        v.push(Variant::progress(format!("progress_bare_50_{th}"), opts("Working (progress bare)", "", "Solving string theory...", XDialogIcon::None, &[]), look(dark))
                   .at(0.3, Action::Progress(TestProgress::Value(0.5)))
                   .capture(1.0, ""));

        // HiDPI smoke set.
        v.push(Variant::message(format!("hidpi2_warning_yesno_{th}"), states_opts("Update Available"), look(dark)).ppp(2.0).capture(1.0, ""));
        v.push(Variant::message(format!("hidpi125_info_ok_{th}"),
                                opts("Information", "Update installed", "My App has been updated to version 2.4.1 successfully.", XDialogIcon::Information, &["OK"]),
                                look(dark)).ppp(1.25)
                                           .capture(1.0, ""));

        // Desktop-portal accent overlay: hover/pressed/focus/progress take the accent, the track is
        // 35 % accent.
        let accented = TestAppearance { accent: Some([0xE9, 0x54, 0x20]), ..look(dark) };
        v.push(Variant::message(format!("accent_hover_{th}"), states_opts("Update Available (accent)"), accented.clone()).at(1.0, Action::HoverButton(no))
                                                                                                                       .capture(1.5, ""));
        v.push(Variant::message(format!("accent_focus_{th}"), states_opts("Update Available (accent)"), accented.clone()).capture(1.0, ""));
        v.push(Variant::progress(format!("accent_progress_{th}"), progress_opts("Downloading (accent)", &[]), accented).at(0.3, Action::Progress(TestProgress::Value(0.5)))
                                                                                                                     .capture(1.0, ""));
    }
    v
}
