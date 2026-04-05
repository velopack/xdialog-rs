use xdialog::{show_progress, XDialogIcon};

fn main() {
    xdialog::XDialogBuilder::new().run(run);
}

fn run() {
    // Basic emoji in all fields
    let _ = xdialog::show_message_ok_cancel(
        "🌍 Unicode Test",
        "Hello from around the world! 👋",
        "English: Hello\nSpanish: ¡Hola!\nFrench: Bonjour\nGerman: Grüße\nJapanese: こんにちは\nChinese: 你好世界\nKorean: 안녕하세요\nArabic: مرحبا\nHebrew: שלום\nThai: สวัสดี\nHindi: नमस्ते\nRussian: Привет мир",
        XDialogIcon::Information,
    ).unwrap();

    // Heavy emoji usage
    let _ = xdialog::show_message_ok_cancel(
        "🎨 Emoji Stress Test",
        "📊 Status: All systems operational ✅",
        "Smileys: 😀😃😄😁😆😅🤣😂🙂🙃😉😊😇\n\
         Gestures: 👍👎👌✌️🤞🤟🤙👏🙌👐🤲🤝\n\
         Animals: 🐶🐱🐭🐹🐰🦊🐻🐼🐨🐯🦁🐮\n\
         Food: 🍕🍔🍟🌭🍿🧂🥓🥚🍳🧈🥞🧇\n\
         Flags: 🇺🇸🇬🇧🇫🇷🇩🇪🇯🇵🇰🇷🇨🇳🇧🇷🇮🇳🇷🇺🇦🇺🇨🇦\n\
         Symbols: ♠️♣️♥️♦️🔔🎵🎶💡🔋🔌💻🖥️\n\
         Math: ∑∏∫∂√∞≈≠≤≥±×÷",
        XDialogIcon::Information,
    ).unwrap();

    // CJK characters and special scripts
    let _ = xdialog::show_message_ok_cancel(
        "漢字テスト — CJK Test",
        "日本語・中文・한국어の表示テスト",
        "Japanese Hiragana: あいうえお かきくけこ さしすせそ\n\
         Japanese Katakana: アイウエオ カキクケコ サシスセソ\n\
         Japanese Kanji: 東京都港区六本木ヒルズ\n\
         Chinese Simplified: 中华人民共和国万岁\n\
         Chinese Traditional: 國立故宮博物院\n\
         Korean: 대한민국 서울특별시\n\
         Vietnamese: Xin chào thế giới\n\
         Bopomofo: ㄅㄆㄇㄈㄉㄊㄋㄌ",
        XDialogIcon::Information,
    ).unwrap();

    // Special Unicode characters and symbols
    let _ = xdialog::show_message_ok_cancel(
        "♻️ Special Characters",
        "Currency & Typography Test 💰",
        "Currency: $ € £ ¥ ₹ ₽ ₿ ₩ ₫ ₴ ₸ ¢\n\
         Typography: \u{00AB}\u{00BB} \u{201E}\u{201C} \u{201A}\u{2018} \u{201C}\u{201D} \u{2018}\u{2019} — – … · • ° ™ © ® ¶ §\n\
         Arrows: ← → ↑ ↓ ↔ ↕ ⇐ ⇒ ⇑ ⇓ ➡️ ⬅️ ⬆️ ⬇️\n\
         Box Drawing: ┌─┬─┐ │ │ │ ├─┼─┤ └─┴─┘\n\
         Braille: ⠓⠑⠇⠇⠕ ⠺⠕⠗⠇⠙\n\
         Dingbats: ✓ ✗ ✦ ✧ ★ ☆ ✿ ❀ ❁ ❂ ❃ ❄\n\
         Musical: 𝄞 𝄢 ♩ ♪ ♫ ♬ 🎼 🎵 🎶",
        XDialogIcon::Warning,
    ).unwrap();

    // Combining characters, accents, and edge cases
    let _ = xdialog::show_message_ok_cancel(
        "Ünïcödé Édgé Çàsés",
        "Z̤͔ͧ̑a̧͚ͨl̖̺͑̆g̩̜̀o̱̺ͬ and Friends",
        "Accented: àáâãäå èéêë ìíîï òóôõöø ùúûü ýÿ ñ ç ß\n\
         Ligatures: ﬀ ﬁ ﬂ ﬃ ﬄ Æ æ Œ œ\n\
         Superscript: ⁰¹²³⁴⁵⁶⁷⁸⁹ⁿ\n\
         Subscript: ₀₁₂₃₄₅₆₇₈₉\n\
         Greek: αβγδεζηθικλμνξοπρστυφχψω\n\
         Cyrillic: абвгдежзийклмнопрстуфхцчшщъыьэюя\n\
         IPA: ɪntəˈnæʃənəl fəˈnɛtɪk ˈælfəbɛt\n\
         Zero-width: [a\u{200B}b\u{200C}c\u{200D}d\u{FEFF}e] ← has invisible chars",
        XDialogIcon::Error,
    ).unwrap();

    // RTL text and bidirectional content
    let _ = xdialog::show_message_ok_cancel(
        "🔄 Bidirectional Text",
        "Right-to-Left & Mixed Direction",
        "Arabic: بسم الله الرحمن الرحيم\n\
         Hebrew: בְּרֵאשִׁית בָּרָא אֱלֹהִים\n\
         Mixed: The word شمس means sun\n\
         Persian: زبان فارسی بسیار زیباست\n\
         Urdu: اردو پاکستان کی قومی زبان ہے\n\
         Devanagari: ॐ नमः शिवाय\n\
         Tamil: தமிழ் நாடு\n\
         Georgian: საქართველო",
        XDialogIcon::Information,
    ).unwrap();

    // Skin tone modifiers and complex emoji sequences
    let data = xdialog::XDialogOptions {
        icon: XDialogIcon::Information,
        title: "👨‍👩‍👧‍👦 Complex Emoji".to_string(),
        main_instruction: "Family & Skin Tone Modifiers 🏽".to_string(),
        message: "Skin tones: 👋🏻👋🏼👋🏽👋🏾👋🏿\n\
                  Families: 👨‍👩‍👧‍👦 👩‍👩‍👦‍👦 👨‍👨‍👧‍👧\n\
                  Professions: 👩‍🔬👨‍🚀👩‍💻👨‍🍳👩‍🎤\n\
                  Compound: 🏳️‍🌈 🏴‍☠️ 🐻‍❄️\n\
                  Keycaps: 0️⃣1️⃣2️⃣3️⃣4️⃣5️⃣6️⃣7️⃣8️⃣9️⃣🔟\n\
                  Clock faces: 🕐🕑🕒🕓🕔🕕🕖🕗🕘🕙🕚🕛"
            .to_string(),
        buttons: vec!["Looks Good! 👍".to_string(), "Broken 💔".to_string()],
    };
    let _ = xdialog::show_message(data, None);

    // Progress dialog with unicode
    let d = show_progress(
        "⏳ Загрузка / ダウンロード中",
        "Downloading 下載中 📦",
        "Processing: données → データ → 数据 → 데이터",
        XDialogIcon::Information,
    )
    .unwrap();
    d.set_indeterminate().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3));
    d.set_value(50.0).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(2));
    d.set_value(100.0).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));
    d.close().unwrap();
}
