use std::fs;
use std::path::PathBuf;

use chrono::NaiveDate;
use iced::keyboard::{Key, Modifiers, key};
use iced::mouse::ScrollDelta;
use iced::widget::text_editor::{Action, Edit};
use iced::{Size, window};

use tsuratsura::app::{App, Message, PickerKind};
use tsuratsura::storage::daily_path;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dwm-app-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        let _ = app.update(Message::Edit(Action::Edit(Edit::Insert(c))));
    }
}

#[test]
fn rollover_flushes_old_day_and_opens_new_day() {
    let dir = temp_dir("rollover");
    let data_dir = dir.join("data");
    let mut app = App::with_paths(&dir.join("config.toml"), &data_dir, d(2026, 12, 31));
    type_str(&mut app, "大晦日のメモ");

    app.check_date(d(2027, 1, 1));

    assert_eq!(app.active_date(), d(2027, 1, 1));
    assert_eq!(app.text(), "");
    assert_eq!(
        fs::read_to_string(daily_path(&data_dir, d(2026, 12, 31))).unwrap(),
        "大晦日のメモ"
    );
    assert!(daily_path(&data_dir, d(2027, 1, 1)).exists());
}

#[test]
fn rollover_loads_existing_file_for_today() {
    let dir = temp_dir("existing");
    let data_dir = dir.join("data");
    let today = daily_path(&data_dir, d(2026, 10, 1));
    fs::create_dir_all(today.parent().unwrap()).unwrap();
    fs::write(&today, "既存\n").unwrap();

    let mut app = App::with_paths(&dir.join("config.toml"), &data_dir, d(2026, 9, 30));
    app.check_date(d(2026, 10, 1));

    assert_eq!(app.text(), "既存\n");
}

#[test]
fn same_day_focus_keeps_content() {
    let dir = temp_dir("same-day");
    let mut app = App::with_paths(&dir.join("config.toml"), &dir.join("data"), d(2026, 9, 30));
    type_str(&mut app, "abc");

    app.check_date(d(2026, 9, 30));

    assert_eq!(app.active_date(), d(2026, 9, 30));
    assert_eq!(app.text(), "abc");
}

#[test]
fn rollover_is_blocked_when_old_day_cannot_be_saved() {
    let dir = temp_dir("blocked");
    let data_dir = dir.join("data");
    let mut app = App::with_paths(&dir.join("config.toml"), &data_dir, d(2026, 9, 30));
    type_str(&mut app, "未保存");

    // 保存先ディレクトリをファイルに置き換えて書き込めなくする
    fs::remove_dir_all(data_dir.join("daily")).unwrap();
    fs::write(data_dir.join("daily"), "blocker").unwrap();
    app.check_date(d(2026, 10, 1));

    assert_eq!(app.active_date(), d(2026, 9, 30));
    assert_eq!(app.text(), "未保存");
}

#[test]
fn crlf_paste_is_normalized() {
    let dir = temp_dir("paste");
    let mut app = App::with_paths(&dir.join("config.toml"), &dir.join("data"), d(2026, 9, 30));
    let _ = app.update(Message::Edit(Action::Edit(Edit::Paste(
        "a\r\nb\r\n".to_owned().into(),
    ))));

    assert_eq!(app.text(), "a\nb\n");
}

#[test]
fn prefix_picker_inserts_at_line_start_and_undoes_in_one_step() {
    let dir = temp_dir("prefix");
    let mut app = App::with_paths(&dir.join("config.toml"), &dir.join("data"), d(2026, 9, 30));
    type_str(&mut app, "折り返し連絡する");

    let _ = app.update(Message::OpenPicker(PickerKind::Prefix));
    let _ = app.update(Message::Choose(1));
    assert_eq!(app.text(), "<remind> 折り返し連絡する");

    // セレクタが閉じた後の選択は何も挿入しない
    let _ = app.update(Message::Choose(0));
    assert_eq!(app.text(), "<remind> 折り返し連絡する");

    let _ = app.update(Message::Undo);
    assert_eq!(app.text(), "折り返し連絡する");
}

#[test]
fn entry_picker_still_inserts_header() {
    let dir = temp_dir("entry-picker");
    let mut app = App::with_paths(&dir.join("config.toml"), &dir.join("data"), d(2026, 9, 30));

    let _ = app.update(Message::OpenPicker(PickerKind::Entry));
    let _ = app.update(Message::Choose(0));

    assert!(app.text().ends_with("] 作業メモ\n"), "{}", app.text());
}

#[test]
fn day_picker_opens_past_day_and_returns_to_today() {
    let dir = temp_dir("past");
    let data_dir = dir.join("data");
    let past = write_day(&data_dir, d(2026, 9, 28));
    write_day(&data_dir, d(2026, 9, 29));
    let mut app = App::with_paths(&dir.join("config.toml"), &data_dir, d(2026, 9, 30));
    type_str(&mut app, "今日のメモ");

    // 一覧は 今日, 09-29, 09-28 の順
    let _ = app.update(Message::OpenPicker(PickerKind::Day));
    let _ = app.update(Message::Choose(2));
    assert_eq!(app.active_date(), d(2026, 9, 28));
    assert_eq!(app.text(), "過去のメモ");
    assert!(app.title().contains("過去のメモ"));
    assert_eq!(
        fs::read_to_string(daily_path(&data_dir, d(2026, 9, 30))).unwrap(),
        "今日のメモ"
    );

    // 過去の日への編集はその日のファイルへ保存され、日付が変わっても今日へ移らない
    type_str(&mut app, "追記");
    app.check_date(d(2026, 10, 1));
    assert_eq!(app.active_date(), d(2026, 9, 28));

    let _ = app.update(Message::OpenPicker(PickerKind::Day));
    let _ = app.update(Message::Choose(0));
    assert_eq!(app.active_date(), d(2026, 10, 1));
    assert_eq!(app.text(), "");
    assert!(!app.title().contains("過去のメモ"));
    assert_eq!(fs::read_to_string(&past).unwrap(), "過去のメモ追記");

    // 今日へ戻った後は、また日付の切り替えに追従する
    app.check_date(d(2026, 10, 2));
    assert_eq!(app.active_date(), d(2026, 10, 2));
}

#[test]
fn day_picker_does_not_switch_when_current_day_cannot_be_saved() {
    let dir = temp_dir("past-blocked");
    let data_dir = dir.join("data");
    write_day(&data_dir, d(2026, 9, 29));
    let mut app = App::with_paths(&dir.join("config.toml"), &data_dir, d(2026, 9, 30));
    type_str(&mut app, "未保存");
    let _ = app.update(Message::OpenPicker(PickerKind::Day));

    // 一覧を出した後で、保存先ディレクトリをファイルに置き換えて書き込めなくする
    fs::remove_dir_all(data_dir.join("daily")).unwrap();
    fs::write(data_dir.join("daily"), "blocker").unwrap();
    let _ = app.update(Message::Choose(1));
    assert_eq!(app.text(), "未保存");
    assert_eq!(app.active_date(), d(2026, 9, 30));
}

fn write_day(data_dir: &std::path::Path, date: NaiveDate) -> PathBuf {
    let path = daily_path(data_dir, date);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "過去のメモ").unwrap();
    path
}

#[test]
fn old_days_are_purged_on_startup_and_rollover() {
    let dir = temp_dir("purge");
    let data_dir = dir.join("data");
    let expired = write_day(&data_dir, d(2026, 8, 30));
    let expires_next_day = write_day(&data_dir, d(2026, 8, 31));

    let mut app = App::with_paths(&dir.join("config.toml"), &data_dir, d(2026, 9, 30));
    assert!(!expired.exists());
    assert!(expires_next_day.exists());

    app.check_date(d(2026, 10, 1));
    assert!(!expires_next_day.exists());
    assert!(daily_path(&data_dir, d(2026, 9, 30)).exists());
}

#[test]
fn retention_days_zero_keeps_everything() {
    let dir = temp_dir("purge-off");
    let data_dir = dir.join("data");
    let old = write_day(&data_dir, d(2020, 1, 1));
    fs::write(dir.join("config.toml"), "retention_days = 0\n").unwrap();

    let _app = App::with_paths(&dir.join("config.toml"), &data_dir, d(2026, 9, 30));

    assert!(old.exists());
}

#[test]
fn nothing_is_purged_when_config_is_invalid() {
    let dir = temp_dir("purge-bad-config");
    let data_dir = dir.join("data");
    let old = write_day(&data_dir, d(2020, 1, 1));
    fs::write(
        dir.join("config.toml"),
        "retention_days = 365\nfont_size = 999\n",
    )
    .unwrap();

    let _app = App::with_paths(&dir.join("config.toml"), &data_dir, d(2026, 9, 30));

    assert!(old.exists());
}

fn press(app: &mut App, key: Key, code: key::Code, modifiers: Modifiers) {
    let _ = app.update(Message::KeyPressed(
        key,
        key::Physical::Code(code),
        modifiers,
    ));
}

#[test]
fn pin_starts_from_config_and_toggles() {
    let dir = temp_dir("pin");
    fs::write(dir.join("config.toml"), "always_on_top = true\n").unwrap();
    let mut app = App::with_paths(&dir.join("config.toml"), &dir.join("data"), d(2026, 9, 30));
    assert!(app.is_pinned());

    let _ = app.update(Message::TogglePin);
    assert!(!app.is_pinned());

    press(
        &mut app,
        Key::Character("T".into()),
        key::Code::KeyT,
        Modifiers::COMMAND | Modifiers::SHIFT,
    );
    assert!(app.is_pinned());
}

#[test]
fn collapse_keeps_text_and_expands_only_on_request() {
    let dir = temp_dir("collapse");
    let mut app = App::with_paths(&dir.join("config.toml"), &dir.join("data"), d(2026, 9, 30));
    assert!(!app.is_pinned());
    type_str(&mut app, "書きかけ");

    let _ = app.update(Message::Collapse(Size::new(720.0, 640.0)));
    assert!(app.is_collapsed());
    assert_eq!(app.text(), "書きかけ");

    press(
        &mut app,
        Key::Named(key::Named::Enter),
        key::Code::Enter,
        Modifiers::empty(),
    );
    assert!(!app.is_collapsed());

    // フォーカスの出入りでは畳みも開きもしない
    let _ = app.update(Message::TogglePin);
    let _ = app.update(Message::WindowUnfocused);
    let _ = app.update(Message::WindowFocused);
    assert!(!app.is_collapsed());
    let _ = app.update(Message::Collapse(Size::new(720.0, 640.0)));
    let _ = app.update(Message::WindowUnfocused);
    let _ = app.update(Message::WindowFocused);
    assert!(app.is_collapsed());

    let _ = app.update(Message::Expand);
    assert!(!app.is_collapsed());

    press(
        &mut app,
        Key::Character("m".into()),
        key::Code::KeyM,
        Modifiers::COMMAND | Modifiers::SHIFT,
    );
    let _ = app.update(Message::Collapse(Size::new(720.0, 640.0)));
    press(
        &mut app,
        Key::Character("m".into()),
        key::Code::KeyM,
        Modifiers::COMMAND | Modifiers::SHIFT,
    );
    assert!(!app.is_collapsed());
}

#[test]
fn closing_collapsed_window_with_unsaved_text_keeps_it_open() {
    let dir = temp_dir("collapse-close");
    let data_dir = dir.join("data");
    let mut app = App::with_paths(&dir.join("config.toml"), &data_dir, d(2026, 9, 30));
    type_str(&mut app, "未保存");
    let _ = app.update(Message::Collapse(Size::new(720.0, 640.0)));

    // 保存先ディレクトリをファイルに置き換えて書き込めなくする
    fs::remove_dir_all(data_dir.join("daily")).unwrap();
    fs::write(data_dir.join("daily"), "blocker").unwrap();
    let _ = app.update(Message::CloseRequested(window::Id::unique()));

    assert!(app.is_collapsed());
    assert_eq!(app.text(), "未保存");
}

#[test]
fn zoom_keys_change_font_size_within_range_and_reset() {
    let dir = temp_dir("zoom-keys");
    fs::write(dir.join("config.toml"), "font_size = 20\n").unwrap();
    let mut app = App::with_paths(&dir.join("config.toml"), &dir.join("data"), d(2026, 9, 30));
    assert_eq!(app.font_size(), 20);

    // `+` は Shift なしの同じキー（US 配列の `=`、JIS 配列の `;`）でも拡大する
    for (key, code) in [
        ("+", key::Code::NumpadAdd),
        ("=", key::Code::Equal),
        (";", key::Code::Semicolon),
    ] {
        press(
            &mut app,
            Key::Character(key.into()),
            code,
            Modifiers::COMMAND,
        );
    }
    assert_eq!(app.font_size(), 23);
    press(
        &mut app,
        Key::Character(";".into()),
        key::Code::Semicolon,
        Modifiers::COMMAND | Modifiers::SHIFT,
    );
    assert_eq!(app.font_size(), 24);

    press(
        &mut app,
        Key::Character("-".into()),
        key::Code::Minus,
        Modifiers::COMMAND,
    );
    assert_eq!(app.font_size(), 23);

    // Primary なしの入力や AltGr（Ctrl+Alt）での記号入力では変えない
    press(
        &mut app,
        Key::Character("-".into()),
        key::Code::Minus,
        Modifiers::empty(),
    );
    press(
        &mut app,
        Key::Character("+".into()),
        key::Code::BracketRight,
        Modifiers::COMMAND | Modifiers::ALT,
    );
    assert_eq!(app.font_size(), 23);

    let _ = app.update(Message::Zoom(100));
    assert_eq!(app.font_size(), 72);
    let _ = app.update(Message::Zoom(-100));
    assert_eq!(app.font_size(), 8);

    press(
        &mut app,
        Key::Character("0".into()),
        key::Code::Digit0,
        Modifiers::COMMAND,
    );
    assert_eq!(app.font_size(), 20);
}

#[test]
fn wheel_zooms_only_while_primary_is_held() {
    let dir = temp_dir("zoom-wheel");
    let mut app = App::with_paths(&dir.join("config.toml"), &dir.join("data"), d(2026, 9, 30));
    let lines = |y| Message::WheelScrolled(ScrollDelta::Lines { x: 0.0, y });
    let pixels = |y| Message::WheelScrolled(ScrollDelta::Pixels { x: 0.0, y });
    assert_eq!(app.font_size(), 15);

    let _ = app.update(lines(1.0));
    assert_eq!(app.font_size(), 15);

    let _ = app.update(Message::ModifiersChanged(Modifiers::COMMAND));
    let _ = app.update(lines(1.0));
    assert_eq!(app.font_size(), 16);
    // 1ノッチが小さな値で届く環境でも1段階ずつ変える
    let _ = app.update(lines(-0.1));
    assert_eq!(app.font_size(), 15);

    // ピクセル単位の入力は、移動量が溜まってから1段階変える
    let _ = app.update(pixels(10.0));
    assert_eq!(app.font_size(), 15);
    let _ = app.update(pixels(30.0));
    assert_eq!(app.font_size(), 16);
    let _ = app.update(pixels(-80.0));
    assert_eq!(app.font_size(), 14);

    // Primary を離すと端数を持ち越さない
    let _ = app.update(pixels(30.0));
    let _ = app.update(Message::ModifiersChanged(Modifiers::empty()));
    let _ = app.update(pixels(30.0));
    let _ = app.update(Message::ModifiersChanged(Modifiers::COMMAND));
    let _ = app.update(pixels(30.0));
    assert_eq!(app.font_size(), 14);
}

#[test]
fn zoom_is_ignored_while_editor_is_hidden() {
    let dir = temp_dir("zoom-hidden");
    let mut app = App::with_paths(&dir.join("config.toml"), &dir.join("data"), d(2026, 9, 30));

    let _ = app.update(Message::Collapse(Size::new(720.0, 640.0)));
    press(
        &mut app,
        Key::Character("=".into()),
        key::Code::Equal,
        Modifiers::COMMAND,
    );
    assert_eq!(app.font_size(), 15);
    let _ = app.update(Message::Expand);

    let _ = app.update(Message::OpenPicker(PickerKind::Entry));
    press(
        &mut app,
        Key::Character("=".into()),
        key::Code::Equal,
        Modifiers::COMMAND,
    );
    assert_eq!(app.font_size(), 15);
    let _ = app.update(Message::ClosePicker);

    press(
        &mut app,
        Key::Character("=".into()),
        key::Code::Equal,
        Modifiers::COMMAND,
    );
    assert_eq!(app.font_size(), 16);
}
