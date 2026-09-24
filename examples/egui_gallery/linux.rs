//! Linux (skia-look) theme gallery variants, 1:1 with the skia reference captures.
//!
//! Names are the skia reference base names plus `_light` / `_dark`; each variant's reference is
//! `skia/<name>.png` (client-area captures at scale 1.0), animation frames `skia/<name>_fNN.png`.
//! Strings and scripts reproduce `examples/refcapture.rs` of the capture worktree (INDEX.md
//! "How these were captured"): the state sequences ran on ONE dialog, so each variant replays the
//! earlier steps of its sequence. Frame bursts sample every 16 ms from the triggering event (the
//! reference CSV timestamps); `_fNN` probes also accept frames NN±1 or a colour between them.
//!
//! Tune probes / add variants freely.

use super::*;

const LONG: &str = "At vero eos et accusamus et iusto odio dignissimos ducimus qui blanditiis praesentium voluptatum deleniti atque corrupti quos dolores et quas molestias excepturi sint occaecati cupiditate non provident, similique sunt in culpa qui officia deserunt mollitia animi, id est laborum et dolorum fuga. Et harum quidem rerum facilis est et expedita distinctio. Nam libero tempore, cum soluta nobis est eligendi optio cumque nihil impedit quo minus id quod maxime placeat facere possimus, omnis voluptas assumenda est, omnis dolor repellendus.";

/// skia theme colours `(light, dark)`.
const BG: ([u8; 3], [u8; 3]) = ([0xFA, 0xFA, 0xFA], [0x2D, 0x2D, 0x2D]);
const IDLE_FILL: ([u8; 3], [u8; 3]) = ([0xFF, 0xFF, 0xFF], [0x3B, 0x3B, 0x3B]);
const IDLE_BORDER: ([u8; 3], [u8; 3]) = ([0xC7, 0xC7, 0xC7], [0x5A, 0x5A, 0x5A]);
const ACCENT: [u8; 3] = [0x2A, 0x7D, 0xE3];
const PRESSED: [u8; 3] = [0x1E, 0x5F, 0xAF];
const TRACK: ([u8; 3], [u8; 3]) = ([0xAD, 0xCE, 0xF7], [0x4A, 0x4A, 0x4A]);

fn pick(c: ([u8; 3], [u8; 3]), dark: bool) -> [u8; 3] {
    if dark {
        c.1
    } else {
        c.0
    }
}

/// The refcapture probe points: fill 6 px inside the left border stroke at mid height, border on
/// the top stroke 19 px from the left edge.
fn fill_at(i: usize) -> (Anchor, (f32, f32)) {
    (Anchor::ButtonMid(i), (6.0, 0.0))
}
fn border_at(i: usize) -> (Anchor, (f32, f32)) {
    (Anchor::Button(i), (19.0, 0.0))
}

fn fill(label: &str, i: usize, c: [u8; 3]) -> Probe {
    let (a, at) = fill_at(i);
    Probe::new(format!("{label} fill"), a, at, c, 3)
}
fn border(label: &str, i: usize, c: [u8; 3]) -> Probe {
    let (a, at) = border_at(i);
    Probe::new(format!("{label} border"), a, at, c, 3)
}

/// Reference-sampled fill + border probes for each frame of a 16-frame burst on button `i`.
fn burst_probes(i: usize) -> Vec<Probe> {
    let (fa, fat) = fill_at(i);
    let (ba, bat) = border_at(i);
    vec![Probe::vs_ref("fill vs ref", fa, fat, 3), Probe::vs_ref("border vs ref", ba, bat, 3)]
}

fn bg_probe(dark: bool) -> Probe {
    Probe::new("background", Anchor::Client, (2.0, -2.0), pick(BG, dark), 2)
}

fn reference(name: &str) -> RefSpec {
    RefSpec::new(format!("skia/{name}{{suffix}}.png"), Crop::Full)
}

/// A single-capture message variant (`t = 1.0`).
fn still(name: &str, dark: bool, o: XDialogOptions) -> Variant {
    let full = format!("{name}_{}", theme_word(dark));
    Variant::message(&full, o, look(dark)).capture(1.0, "").reference(reference(&full)).probe(bg_probe(dark))
}

fn states_opts(title: &str) -> XDialogOptions {
    opts(title, "New version available", "Would you like to update to the new version now?", XDialogIcon::Warning, &["No", "Yes"])
}

/// All variants for the linux theme.
pub fn variants() -> Vec<Variant> {
    let mut v = Vec::new();
    for dark in [false, true] {
        let th = theme_word(dark);

        // ---- static dialogs (INDEX "Static dialog variants") ----
        v.push(still("info_ok", dark, opts("Information", "Update installed", "My App has been updated to version 2.4.1 successfully.", XDialogIcon::Information, &["OK"]))
               .probe(border("OK focused", 0, ACCENT))
               .probe(fill("OK focused", 0, pick(IDLE_FILL, dark)))
               .golden());
        v.push(still("warning_yesno", dark, opts("Update Available", "New version available", "Would you like to update to the new version now?", XDialogIcon::Warning, &["No", "Yes"]))
               .probe(border("No idle", 0, pick(IDLE_BORDER, dark)))
               .probe(border("Yes focused", 1, ACCENT))
               .golden());
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

        // ---- button states (INDEX "Button states"; one dialog, sequential steps) ----
        // Timeline of the reference run: t=1.0 hover Yes; 1.5 hover No; 2.0 move to (5,5);
        // 2.5 back on No; 2.8 press No; 3.3 move off + release; 3.7 capture.
        let st = |name: &str| {
            let full = format!("{name}_{th}");
            Variant::message(&full, states_opts("Update Available (states)"), look(dark)).reference(reference(&full)).probe(bg_probe(dark))
        };
        let (no, yes) = (0usize, 1usize);
        v.push(st("state_idle").capture(1.0, "").probe(border("Yes focused", yes, ACCENT)).probe(border("No idle", no, pick(IDLE_BORDER, dark))));
        v.push(st("hover_default").at(1.0, Action::HoverButton(yes))
                                  .burst(1.0, 16, 0.016)
                                  .capture(1.5, "")
                                  .probes(burst_probes(yes))
                                  .probe(fill("Yes hovered", yes, ACCENT).only(""))
                                  .probe(fill("Yes start", yes, pick(IDLE_FILL, dark)).only("_f00")));
        let hover_secondary = |v: Variant| v.at(1.0, Action::HoverButton(yes)).at(1.5, Action::HoverButton(no));
        v.push(hover_secondary(st("hover_secondary")).burst(1.5, 16, 0.016)
                                                    .capture(2.0, "")
                                                    .probes(burst_probes(no))
                                                    .probe(fill("No hovered", no, ACCENT).only(""))
                                                    .probe(border("No hovered", no, ACCENT).only(""))
                                                    .probe(border("Yes ring hidden", yes, pick(IDLE_BORDER, dark)).only("")));
        let unhover = |v: Variant| hover_secondary(v).at(2.0, Action::MoveTo(5.0, 5.0));
        v.push(unhover(st("unhover")).burst(2.0, 16, 0.016)
                                    .capture(2.5, "")
                                    .probes(burst_probes(no))
                                    .probe(fill("No idle", no, pick(IDLE_FILL, dark)).only(""))
                                    .probe(border("Yes ring back", yes, ACCENT).only("")));
        let pressed = |v: Variant| unhover(v).at(2.5, Action::HoverButton(no)).at(2.8, Action::PressButton(no));
        v.push(pressed(st("pressed_secondary")).burst(2.8, 16, 0.016)
                                              .capture(3.3, "")
                                              .probes(burst_probes(no))
                                              .probe(fill("No pressed", no, PRESSED).only("")));
        v.push(pressed(st("after_press_focus_moved")).at(3.3, Action::MoveTo(5.0, 5.0))
                                                    .at(3.3, Action::Release)
                                                    .capture(3.7, "")
                                                    .probe(border("No focused", no, ACCENT))
                                                    .probe(border("Yes unfocused", yes, pick(IDLE_BORDER, dark)))
                                                    .probe(fill("No idle fill", no, pick(IDLE_FILL, dark))));

        // Keyboard focus (a second dialog, "(focus)").
        let fo = |name: &str| {
            let full = format!("{name}_{th}");
            Variant::message(&full, states_opts("Update Available (focus)"), look(dark)).reference(reference(&full)).probe(bg_probe(dark))
        };
        v.push(fo("focus_default").capture(1.0, "").probe(border("Yes focused", yes, ACCENT)).golden());
        // The focus_tab references start 15.1 ms after the key (both CSVs), not at the key.
        v.push(fo("focus_tab").at(1.0, Action::Key(Key::Tab))
                              .burst(1.0151, 16, 0.016)
                              .capture(1.5, "")
                              .probes(burst_probes(no))
                              .probe(border("No focused", no, ACCENT).only(""))
                              .probe(border("Yes unfocused", yes, pick(IDLE_BORDER, dark)).only("")));
        v.push(fo("focus_right_arrow").at(1.0, Action::Key(Key::Tab))
                                      .at(1.5, Action::Key(Key::ArrowRight))
                                      .capture(1.8, "")
                                      .probe(border("Yes focused", yes, ACCENT))
                                      .probe(border("No unfocused", no, pick(IDLE_BORDER, dark))));

        // ---- progress (INDEX "Progress"; one dialog, sequential) ----
        // Track: x 80..333, bar-centre row 56 (logical, info icon + title + 1-line body).
        let track = |label: &str, x: f32, c: [u8; 3]| Probe::new(label, Anchor::Client, (x, 56.0), c, 3);
        let pr = |name: &str| {
            let full = format!("{name}_{th}");
            Variant::progress(&full,
                              opts("Downloading (progress)", "Downloading updates", "Downloading package 1 of 3...", XDialogIcon::Information, &[]),
                              look(dark)).reference(reference(&full))
                                         .probe(bg_probe(dark))
        };
        // Timeline: t=1.0 value 0.5; 1.6 value 1.0; 2.4 indeterminate; 2.956 cycle burst start;
        // 6.7 set_text (long) -> capture 7.2.
        v.push(pr("progress_0").capture(1.0, "").probe(track("track", 200.0, pick(TRACK, dark))).golden());
        let to50 = |v: Variant| v.at(1.0, Action::Progress(TestProgress::Value(0.5)));
        v.push(to50(pr("progress_anim_0_to_50")).burst(1.0, 24, 0.016).probe(Probe::vs_ref("bar @ x=100", Anchor::Client, (100.0, 56.0), 3)));
        v.push(to50(pr("progress_50")).capture(1.5, "").probe(track("bar", 150.0, ACCENT)).probe(track("track", 250.0, pick(TRACK, dark))));
        let to100 = |v: Variant| to50(v).at(1.6, Action::Progress(TestProgress::Value(1.0)));
        v.push(to100(pr("progress_100")).capture(2.3, "").probe(track("bar end", 330.0, ACCENT)));
        let ind = |v: Variant| to100(v).at(2.4, Action::Progress(TestProgress::Indeterminate));
        v.push(ind(pr("progress_indeterminate")).burst(2.4, 12, 0.05));
        // Positions only in the reference (`.csv` + `_strip.png`, no per-frame PNGs). The reference
        // burst started 0.54 s after the restart, which lands at 2.416 here (restart + one tick):
        // a phase fit of the 110 CSV samples gives offsets of 0.542 s (light) / 0.536 s (dark).
        let mut cycle = ind(pr("progress_indeterminate_cycle")).burst(2.956, 110, 0.033);
        cycle.reference = None;
        v.push(cycle);
        v.push(ind(pr("progress_indeterminate_longtext")).at(6.7,
                                                            Action::SetText("This is some long text which should wrap and cause the window size to be re-calculated. \
                                                                             This is some long text which should wrap."
                                                                                                                         .into()))
                                                        .capture(7.2, ""));

        // Progress with a Cancel button, then hovered.
        let pc = |name: &str| {
            let full = format!("{name}_{th}");
            Variant::progress(&full,
                              opts("Downloading (cancel)", "Downloading updates", "Downloading package 2 of 3...", XDialogIcon::Information, &["Cancel"]),
                              look(dark)).reference(reference(&full))
                                         .probe(bg_probe(dark))
                                         .at(0.3, Action::Progress(TestProgress::Value(0.35)))
        };
        v.push(pc("progress_cancel").capture(1.0, "").probe(border("Cancel focused", 0, ACCENT)));
        v.push(pc("progress_cancel_hover").at(1.0, Action::HoverButton(0)).capture(1.3, "").probe(fill("Cancel hovered", 0, ACCENT)));

        // No icon, no title.
        let full = format!("progress_bare_50_{th}");
        v.push(Variant::progress(&full, opts("Working (progress bare)", "", "Solving string theory...", XDialogIcon::None, &[]), look(dark))
                   .at(0.3, Action::Progress(TestProgress::Value(0.5)))
                   .capture(1.0, "")
                   .reference(reference(&full))
                   .probe(bg_probe(dark))
                   .probe(Probe::new("bar", Anchor::Client, (40.0, 19.0), ACCENT, 3)));
    }

    // Behaviour where skia has no capture (no references, probes only).
    for dark in [false, true] {
        let th = theme_word(dark);
        let (no, yes) = (0usize, 1usize);
        // Pointer leaves the window over the hovered "No" (a documented deviation: skia ignored
        // `CursorLeft`): "No" fades back to idle and the focus ring on "Yes" returns.
        v.push(Variant::message(format!("leave_restores_focus_{th}"), states_opts("Update Available (leave)"), look(dark))
                   .at(1.0, Action::HoverButton(no))
                   .at(1.5, Action::Leave)
                   .capture(1.3, "_hovered")
                   .capture(2.0, "")
                   .probe(bg_probe(dark))
                   .probe(fill("No hovered", no, ACCENT).only("_hovered"))
                   .probe(border("Yes ring hidden", yes, pick(IDLE_BORDER, dark)).only("_hovered"))
                   .probe(fill("No idle", no, pick(IDLE_FILL, dark)).only(""))
                   .probe(border("Yes ring back", yes, ACCENT).only("")));
        // An inactive window keeps the default button's focus ring (skia ignored window focus).
        v.push(Variant::message(format!("inactive_focus_{th}"), states_opts("Update Available (inactive)"), look(dark))
                   .at(0.5, Action::WindowFocus(false))
                   .capture(1.0, "")
                   .probe(bg_probe(dark))
                   .probe(border("Yes focused", yes, ACCENT))
                   .probe(border("No idle", no, pick(IDLE_BORDER, dark))));
        // Right-to-left paragraphs end at the text column's right edge (cosmic-text alignment),
        // not at the widest line's: the one-line Arabic title leaves the left of the column empty.
        v.push(Variant::message(format!("rtl_align_{th}"),
                                opts("QA2 rtl",
                                     "مرحبا بالعالم",
                                     "هذا نص عربي طويل للتحقق من الالتفاف والمحاذاة في مربع الحوار. שלום עולם, זהו טקסט בעברית.",
                                     XDialogIcon::Information,
                                     &["إلغاء", "موافق"]),
                                look(dark)).capture(1.0, "")
                                           .probe(bg_probe(dark))
                                           .probes([90.0, 120.0, 150.0, 180.0, 210.0].map(|x| Probe::new(format!("title left empty x={x}"), Anchor::Client, (x, 30.0), pick(BG, dark), 2)).to_vec()));
    }

    // HiDPI smoke set (no references: the skia captures are scale 1.0 only).
    for dark in [false, true] {
        let th = theme_word(dark);
        v.push(Variant::message(format!("hidpi2_warning_yesno_{th}"),
                                opts("Update Available", "New version available", "Would you like to update to the new version now?", XDialogIcon::Warning, &["No", "Yes"]),
                                look(dark)).ppp(2.0)
                                           .capture(1.0, "")
                                           .probe(bg_probe(dark))
                                           .probe(border("Yes focused", 1, ACCENT)));
        v.push(Variant::message(format!("hidpi125_info_ok_{th}"),
                                opts("Information", "Update installed", "My App has been updated to version 2.4.1 successfully.", XDialogIcon::Information, &["OK"]),
                                look(dark)).ppp(1.25)
                                           .capture(1.0, "")
                                           .probe(bg_probe(dark)));
    }

    // Desktop-portal accent overlay (skia `apply_accent`; not captured, skia's detection is a
    // no-op off Linux): hover/pressed/focus/progress take the accent, the track is 35 % accent.
    const ORANGE: [u8; 3] = [0xE9, 0x54, 0x20];
    for dark in [false, true] {
        let th = theme_word(dark);
        let accented = TestAppearance { accent: Some(ORANGE), ..look(dark) };
        let bg = pick(BG, dark);
        let track = [0, 1, 2].map(|i| (ORANGE[i] as f32 * 0.35 + bg[i] as f32 * 0.65).round() as u8);
        v.push(Variant::message(format!("accent_hover_{th}"), states_opts("Update Available (accent)"), accented.clone())
                   .at(1.0, Action::HoverButton(0))
                   .capture(1.5, "")
                   .probe(bg_probe(dark))
                   .probe(fill("No hovered", 0, ORANGE))
                   .probe(border("Yes ring hidden", 1, pick(IDLE_BORDER, dark))));
        v.push(Variant::message(format!("accent_focus_{th}"), states_opts("Update Available (accent)"), accented.clone())
                   .capture(1.0, "")
                   .probe(border("Yes focused", 1, ORANGE))
                   .probe(fill("Yes fill", 1, pick(IDLE_FILL, dark))));
        v.push(Variant::progress(format!("accent_progress_{th}"),
                                 opts("Downloading (accent)", "Downloading updates", "Downloading package 1 of 3...", XDialogIcon::Information, &[]),
                                 accented).at(0.3, Action::Progress(TestProgress::Value(0.5)))
                                          .capture(1.0, "")
                                          .probe(Probe::new("bar", Anchor::Client, (150.0, 56.0), ORANGE, 3))
                                          .probe(Probe::new("track", Anchor::Client, (250.0, 56.0), track, 3)));
    }
    v
}
