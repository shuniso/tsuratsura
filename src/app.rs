//! アプリ状態・メッセージ・更新処理。

use std::path::PathBuf;
use std::time::Duration;

use chrono::{Local, NaiveDate};
use iced::advanced::widget::{operate, operation::focusable};
use iced::event::{self, Event};
use iced::keyboard::{self, Key, key};
use iced::widget::scrollable::RelativeOffset;
use iced::widget::text_editor::{self, Binding, KeyPress, Status};
use iced::widget::{button, column, container, mouse_area, operation, row, space, stack, text};
use iced::{Color, Element, Fill, Size, Subscription, Task, mouse, window};

use crate::config::{self, APP_ID, Config, FONT_SIZE_RANGE};
use crate::editor::{self, History};
use crate::{date, entry, saver, storage, ui};

const EDITOR_ID: &str = "editor";

/// 折り畳んだ時のウィンドウの内寸。
const BAR_SIZE: Size = Size::new(260.0, 28.0);

/// ピクセル単位のホイール入力（トラックパッドなど）で、フォントサイズを1段階変える移動量。
const ZOOM_PIXELS_PER_STEP: f32 = 40.0;

/// セレクタの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    Entry,
    Prefix,
    /// 過去の日次メモ。
    Day,
}

pub struct App {
    config: Config,
    data_dir: PathBuf,
    active_date: NaiveDate,
    /// 最後に確認した今日の日付。
    today: NaiveDate,
    /// 今日以外の日を選んで開いている。日付が変わっても今日へ移らない。
    viewing_past: bool,
    /// 過去メモのセレクタに出している日付（今日が先頭、以降は新しい順）。
    days: Vec<NaiveDate>,
    path: PathBuf,
    content: text_editor::Content,
    history: History,
    /// 開いている時は種類と選択中の行番号。
    picker: Option<(PickerKind, usize)>,
    /// 編集ごとに増える版番号。`saved_rev` と一致すれば保存済み。
    rev: u64,
    saved_rev: u64,
    saver: Option<saver::Handle>,
    /// 日次ファイルを読めなかった。上書きを防ぐため保存しない。
    load_failed: bool,
    /// config で指定された本文フォント。`None` なら同梱フォント。
    editor_font: Option<iced::Font>,
    config_error: Option<String>,
    load_error: Option<String>,
    save_error: Option<String>,
    /// 未保存のまま閉じようとして警告済み。もう一度閉じると終了する。
    close_armed: bool,
    /// Primary キーを押している。
    primary_held: bool,
    /// ウィンドウを最前面に固定している。
    pinned: bool,
    /// 小さなバーへ折り畳んでいる。
    collapsed: bool,
    /// 折り畳む前のウィンドウの内寸。展開時に戻す。
    expanded_size: Size,
    /// 本文のフォントサイズ。config の値から始まり、ズームで変わる（保存しない）。
    font_size: u16,
    /// ピクセル単位のホイール入力のうち、まだズームに使っていない端数。
    zoom_remainder: f32,
}

#[derive(Debug, Clone)]
pub enum Message {
    Edit(text_editor::Action),
    OpenPicker(PickerKind),
    ClosePicker,
    Choose(usize),
    /// どのウィジェットも処理しなかったキー入力。
    KeyPressed(Key, keyboard::key::Physical, keyboard::Modifiers),
    TogglePin,
    ToggleCollapse,
    /// 現在の内寸を覚えて折り畳む。
    Collapse(Size),
    Expand,
    /// 本文のフォントサイズを指定した段階だけ変える。
    Zoom(i16),
    ZoomReset,
    WheelScrolled(mouse::ScrollDelta),
    Undo,
    Redo,
    CopyAll,
    Flush,
    ModifiersChanged(keyboard::Modifiers),
    WindowOpened,
    WindowFocused,
    WindowUnfocused,
    CloseRequested(window::Id),
    Saver(saver::Event),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let (config_path, default_data_dir) = config::paths();

        (
            Self::with_paths(&config_path, &default_data_dir, date::today()),
            operation::focus(EDITOR_ID),
        )
    }

    /// config と既定データディレクトリを指定して起動状態を作る。
    pub fn with_paths(
        config_path: &std::path::Path,
        default_data_dir: &std::path::Path,
        today: NaiveDate,
    ) -> Self {
        let (config, config_error) = config::load(config_path);
        if let Some(e) = &config_error {
            eprintln!("[{APP_ID}] {} ({e})", config_path.display());
        }
        let data_dir = config.resolve_data_dir(default_data_dir);
        // Font は 'static な名前を要求する。config は起動時に一度しか読まないのでリークさせる
        let editor_font = (!config.font_family.trim().is_empty()).then(|| {
            iced::Font::with_name(Box::leak(
                config.font_family.trim().to_owned().into_boxed_str(),
            ))
        });
        let active_date = today;
        let pinned = config.always_on_top;
        let font_size = config.font_size;

        let mut app = Self {
            config,
            path: storage::daily_path(&data_dir, active_date),
            data_dir,
            active_date,
            today,
            viewing_past: false,
            days: Vec::new(),
            content: text_editor::Content::new(),
            history: History::default(),
            picker: None,
            rev: 0,
            saved_rev: 0,
            saver: None,
            load_failed: false,
            editor_font,
            config_error: config_error
                .map(|e| format!("{e}（内蔵デフォルトで起動中: {}）", config_path.display())),
            load_error: None,
            save_error: None,
            close_armed: false,
            primary_held: false,
            pinned,
            collapsed: false,
            expanded_size: Size::ZERO,
            font_size,
            zoom_remainder: 0.0,
        };
        app.open_day(active_date);
        app.purge_old_days();
        app
    }

    /// 現在の本文。
    pub fn text(&self) -> String {
        self.content.text()
    }

    pub fn active_date(&self) -> NaiveDate {
        self.active_date
    }

    pub fn title(&self) -> String {
        let date = self.active_date.format("%Y-%m-%d");
        if self.viewing_past {
            format!("つらつら — {date}（過去のメモ）")
        } else {
            format!("つらつら — {date}")
        }
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    pub fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    pub fn font_size(&self) -> u16 {
        self.font_size
    }

    fn is_dirty(&self) -> bool {
        self.rev != self.saved_rev
    }

    fn debounce(&self) -> Duration {
        Duration::from_millis(self.config.autosave_debounce_ms)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // Primary を押したままのホイールはズームに使い、本文は動かさない
            Message::Edit(text_editor::Action::Scroll { .. }) if self.primary_held => {}
            Message::Edit(action) => {
                let action = normalize_paste(action);
                let is_edit = action.is_edit();
                if is_edit {
                    self.history.before_edit(&self.content, &action);
                } else if !matches!(action, text_editor::Action::Scroll { .. }) {
                    self.history.break_group();
                }
                self.content.perform(action);
                if is_edit {
                    self.mark_edited();
                }
            }
            Message::OpenPicker(kind) => {
                if kind == PickerKind::Day {
                    self.list_days();
                }
                let count = self.picker_items(kind).len();
                if count == 0 {
                    return Task::none();
                }
                // 過去メモは、いま開いている日を選んだ状態で開く
                let selected = match kind {
                    PickerKind::Day => self.days.iter().position(|d| *d == self.active_date),
                    _ => None,
                };
                return Task::batch([
                    operate(focusable::unfocus()),
                    self.select(kind, selected.unwrap_or(0), count),
                ]);
            }
            Message::ClosePicker => {
                self.picker = None;
                return operation::focus(EDITOR_ID);
            }
            Message::Choose(index) => return self.choose(index),
            Message::KeyPressed(key, physical, modifiers) => {
                if let Some(message) = app_shortcut(&key, physical, modifiers) {
                    return self.update(message);
                }
                if self.collapsed {
                    if key == Key::Named(key::Named::Enter) {
                        return self.expand();
                    }
                    return Task::none();
                }
                return self.picker_key(key, physical, modifiers);
            }
            Message::TogglePin => {
                self.pinned = !self.pinned;
                let level = self.apply_level();
                // ボタンを押すとエディタのフォーカスが外れるので戻す
                return if self.picker.is_none() {
                    Task::batch([level, operation::focus(EDITOR_ID)])
                } else {
                    level
                };
            }
            Message::ToggleCollapse => {
                return if self.collapsed {
                    self.expand()
                } else {
                    request_collapse()
                };
            }
            Message::Collapse(size) => return self.collapse(size),
            Message::Expand => return self.expand(),
            Message::Zoom(steps) => self.set_font_size(self.font_size.saturating_add_signed(steps)),
            Message::ZoomReset => self.set_font_size(self.config.font_size),
            Message::WheelScrolled(delta) => {
                if self.primary_held {
                    let steps = self.wheel_zoom_steps(delta);
                    self.set_font_size(self.font_size.saturating_add_signed(steps));
                }
            }
            Message::Undo => {
                if let Some(content) = self.history.undo(&self.content) {
                    self.content = content;
                    self.mark_edited();
                }
            }
            Message::Redo => {
                if let Some(content) = self.history.redo(&self.content) {
                    self.content = content;
                    self.mark_edited();
                }
            }
            Message::CopyAll => return iced::clipboard::write(self.content.text()),
            // macOS の Cmd+Q は close request を経ずに終了するため、
            // Cmd を押した時点で未保存分を書き込みに回す（終了時は saver::at_exit で待つ）
            Message::ModifiersChanged(modifiers) => {
                self.primary_held = modifiers.command();
                if self.primary_held {
                    self.flush();
                } else {
                    self.zoom_remainder = 0.0;
                }
            }
            Message::Flush | Message::WindowUnfocused => self.flush(),
            Message::WindowOpened => return self.apply_level(),
            Message::WindowFocused => self.check_date(date::today()),
            Message::CloseRequested(id) => return self.close_requested(id),
            Message::Saver(event) => self.saver_event(event),
        }

        Task::none()
    }

    fn mark_edited(&mut self) {
        self.rev += 1;
        self.close_armed = false;
        if self.primary_held {
            // Cmd を押したままの貼り付け等は、続く Cmd+Q に備えて即保存する
            self.flush();
        } else if let (Some(saver), false) = (&self.saver, self.load_failed) {
            saver.touch(self.debounce());
        }
    }

    fn apply_level(&self) -> Task<Message> {
        let level = if self.pinned {
            window::Level::AlwaysOnTop
        } else {
            window::Level::Normal
        };
        window::latest().and_then(move |id| window::set_level(id, level))
    }

    fn collapse(&mut self, size: Size) -> Task<Message> {
        if self.collapsed {
            return Task::none();
        }
        self.collapsed = true;
        self.expanded_size = size;
        self.picker = None;
        window::latest().and_then(|id| window::resize(id, BAR_SIZE))
    }

    fn expand(&mut self) -> Task<Message> {
        if !self.collapsed {
            return Task::none();
        }
        self.collapsed = false;
        let size = self.expanded_size;
        Task::batch([
            window::latest().and_then(move |id| window::resize(id, size)),
            operation::focus(EDITOR_ID),
        ])
    }

    /// 本文のフォントサイズを変える。本文が見えていない折り畳み中・セレクタ表示中は変えない。
    fn set_font_size(&mut self, size: u16) {
        if self.collapsed || self.picker.is_some() {
            return;
        }
        self.font_size = size.clamp(*FONT_SIZE_RANGE.start(), *FONT_SIZE_RANGE.end());
    }

    /// ホイール入力をフォントサイズの段階数にする。上へ回すと拡大。
    fn wheel_zoom_steps(&mut self, delta: mouse::ScrollDelta) -> i16 {
        match delta {
            // 行単位の値は環境で大きさが違う（macOS のマウスは1ノッチが 0.1 程度）ので、向きだけ使う
            mouse::ScrollDelta::Lines { y, .. } if y > 0.0 => 1,
            mouse::ScrollDelta::Lines { y, .. } if y < 0.0 => -1,
            mouse::ScrollDelta::Lines { .. } => 0,
            mouse::ScrollDelta::Pixels { y, .. } => {
                self.zoom_remainder += y / ZOOM_PIXELS_PER_STEP;
                let steps = self.zoom_remainder.trunc();
                self.zoom_remainder -= steps;
                steps as i16
            }
        }
    }

    /// セレクタの `(キー, 表示名)`。
    fn picker_items(&self, kind: PickerKind) -> Vec<(String, String)> {
        match kind {
            PickerKind::Entry => self
                .config
                .entry_types
                .iter()
                .map(|e| (e.key.clone(), e.label.clone()))
                .collect(),
            PickerKind::Prefix => self
                .config
                .prefixes
                .iter()
                .map(|p| (p.key.clone(), p.text.trim().to_owned()))
                .collect(),
            // 先頭の9件だけ数字キーで選べる
            PickerKind::Day => self
                .days
                .iter()
                .enumerate()
                .map(|(index, day)| {
                    let key = if index < 9 {
                        (index + 1).to_string()
                    } else {
                        String::new()
                    };
                    let label = if *day == self.today {
                        format!("{}  今日", date::label(*day))
                    } else {
                        date::label(*day)
                    };
                    (key, label)
                })
                .collect(),
        }
    }

    /// セレクタの `index` 番目を選択し、一覧をその行が見える位置へ動かす。
    fn select(&mut self, kind: PickerKind, index: usize, count: usize) -> Task<Message> {
        self.picker = Some((kind, index));
        let y = if count > 1 {
            index as f32 / (count - 1) as f32
        } else {
            0.0
        };
        operation::snap_to(ui::picker::LIST_ID, RelativeOffset { x: 0.0, y })
    }

    /// 過去メモのセレクタに出す日付を読み直す。
    fn list_days(&mut self) {
        let listed = storage::list_daily(&self.data_dir).unwrap_or_else(|e| {
            eprintln!("[{APP_ID}] 日次ファイルの一覧を取得できません: {e}");
            Vec::new()
        });
        self.days = std::iter::once(self.today)
            .chain(listed.into_iter().filter(|day| *day != self.today))
            .collect();
    }

    /// 選んだ日の日次ファイルへ移る。いまの日を保存できなければ移らない。
    fn switch_day(&mut self, day: NaiveDate) {
        if day == self.active_date && !self.load_failed {
            return;
        }
        if self.flush_sync().is_err() {
            return;
        }
        self.open_day(day);
        self.viewing_past = day != self.today;
        if !self.viewing_past {
            self.purge_old_days();
        }
    }

    /// 開いているセレクタの `index` 番目を挿入して閉じる。
    fn choose(&mut self, index: usize) -> Task<Message> {
        match self.picker.take() {
            Some((PickerKind::Entry, _)) => {
                if let Some(entry_type) = self.config.entry_types.get(index) {
                    let body = entry::entry_text(
                        &self.config.entry_header,
                        entry_type,
                        Local::now().naive_local(),
                    );
                    self.history.checkpoint(&self.content);
                    editor::insert_entry(&mut self.content, &body);
                    self.mark_edited();
                }
            }
            Some((PickerKind::Prefix, _)) => {
                if let Some(prefix) = self.config.prefixes.get(index) {
                    self.history.checkpoint(&self.content);
                    editor::insert_prefix(&mut self.content, &prefix.text);
                    self.mark_edited();
                }
            }
            Some((PickerKind::Day, _)) => {
                if let Some(day) = self.days.get(index).copied() {
                    self.switch_day(day);
                }
            }
            None => {}
        }
        operation::focus(EDITOR_ID)
    }

    fn picker_key(
        &mut self,
        key: Key,
        physical: keyboard::key::Physical,
        modifiers: keyboard::Modifiers,
    ) -> Task<Message> {
        let Some((kind, selected)) = self.picker else {
            return Task::none();
        };
        let items = self.picker_items(kind);
        let count = items.len();

        match key.as_ref() {
            Key::Named(key::Named::Escape) => return self.update(Message::ClosePicker),
            Key::Named(key::Named::Enter) => return self.choose(selected),
            Key::Named(key::Named::ArrowUp) => {
                return self.select(kind, (selected + count - 1) % count, count);
            }
            Key::Named(key::Named::ArrowDown) => {
                return self.select(kind, (selected + 1) % count, count);
            }
            _ if modifiers.command() || modifiers.control() || modifiers.alt() => {}
            _ => {
                let pressed = key.to_latin(physical).and_then(|c| c.to_lowercase().next());
                if let Some(index) = items
                    .iter()
                    .position(|(key, _)| pressed.is_some() && entry::key_char(key) == pressed)
                {
                    return self.choose(index);
                }
            }
        }

        Task::none()
    }

    /// 未保存の内容があれば保存ワーカーへ書き込みを依頼する。
    fn flush(&mut self) {
        if !self.is_dirty() {
            return;
        }
        if self.load_failed || self.save_error.is_some() {
            self.update_rescue();
        }
        if self.load_failed {
            return;
        }
        if let Some(saver) = &self.saver {
            saver.write(self.path.clone(), self.content.text(), self.rev);
        }
    }

    /// 書き込み完了まで待って保存する。成功すれば保存済みになる。
    fn flush_sync(&mut self) -> Result<(), String> {
        if !self.is_dirty() {
            return Ok(());
        }
        if self.load_failed {
            return Err("日次ファイルを読み込めなかったため保存を停止しています".into());
        }

        let text = self.content.text();
        let result = match &self.saver {
            Some(saver) => saver.write_sync(self.path.clone(), text),
            None => storage::save_atomic(&self.path, &text).map_err(|e| e.to_string()),
        };
        match result {
            Ok(()) => {
                self.saved_rev = self.rev;
                self.save_error = None;
                saver::set_rescue(None);
                Ok(())
            }
            Err(e) => {
                self.set_save_error(&e);
                Err(e)
            }
        }
    }

    /// 保存できていない本文を、予期しない終了時の退避用に預ける。
    fn update_rescue(&self) {
        let name = format!("{}.txt", Local::now().format("%Y-%m-%d-%H%M%S"));
        saver::set_rescue(Some((
            self.data_dir.join("recovery").join(name),
            self.content.text(),
        )));
    }

    fn set_save_error(&mut self, error: &str) {
        eprintln!("[{APP_ID}] 保存に失敗: {} ({error})", self.path.display());
        self.save_error = Some(format!(
            "保存できていません: {error}（{}）。内容は画面上に残っています。次の入力やウィンドウ切り替え時に再試行します。",
            self.path.display()
        ));
        if self.is_dirty() {
            self.update_rescue();
        }
    }

    fn saver_event(&mut self, event: saver::Event) {
        match event {
            saver::Event::Ready(handle) => {
                self.saver = Some(handle);
                self.flush();
            }
            saver::Event::Due => self.flush(),
            saver::Event::Saved { rev } => {
                self.saved_rev = self.saved_rev.max(rev);
                self.save_error = None;
                if !self.is_dirty() {
                    saver::set_rescue(None);
                }
            }
            // より新しい内容の保存が成功済みなら、古い書き込みの失敗は無視する
            saver::Event::Failed { rev, .. } if rev <= self.saved_rev => {}
            saver::Event::Failed { error, .. } => self.set_save_error(&error),
        }
    }

    /// ウィンドウのフォーカス復帰時の日付確認。日付が変わっていれば今日のファイルへ移る。
    ///
    /// 過去のメモを選んで開いている間は移らない。
    pub fn check_date(&mut self, today: NaiveDate) {
        self.today = today;
        if !self.viewing_past && date::needs_rollover(self.active_date, today) {
            if self.flush_sync().is_ok() {
                self.open_day(today);
                self.purge_old_days();
            }
        } else if self.load_failed && !self.is_dirty() {
            // 一時的な読み込み失敗なら、フォーカス復帰時に読み直す
            self.open_day(self.active_date);
        }
    }

    fn open_day(&mut self, date: NaiveDate) {
        self.active_date = date;
        self.path = storage::daily_path(&self.data_dir, date);
        self.history = History::default();
        self.saved_rev = self.rev;
        self.close_armed = false;

        match storage::open_daily(&self.path) {
            Ok(text) => {
                self.content = text_editor::Content::with_text(&text);
                self.content
                    .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                self.load_failed = false;
                self.load_error = None;
            }
            Err(e) => {
                eprintln!("[{APP_ID}] 読み込みに失敗: {} ({e})", self.path.display());
                self.content = text_editor::Content::new();
                self.load_failed = true;
                self.load_error = Some(format!(
                    "日次ファイルを読み込めません: {e}（{}）。上書きを防ぐため保存を停止しています。",
                    self.path.display()
                ));
            }
        }
    }

    /// 保持日数を過ぎた日次ファイルを削除する。
    fn purge_old_days(&self) {
        // config を読めていない時は保持日数が内蔵デフォルトに戻っているため、削除しない
        if self.config.retention_days == 0 || self.config_error.is_some() {
            return;
        }
        if let Err(e) =
            storage::purge_old_daily(&self.data_dir, self.active_date, self.config.retention_days)
        {
            eprintln!("[{APP_ID}] 古い日次ファイルの削除に失敗: {e}");
        }
    }

    fn close_requested(&mut self, id: window::Id) -> Task<Message> {
        if self.flush_sync().is_ok() || self.close_armed {
            return window::close(id).chain(iced::exit());
        }

        self.close_armed = true;
        Task::none()
    }

    fn has_notice(&self) -> bool {
        self.config_error.is_some()
            || self.load_error.is_some()
            || self.save_error.is_some()
            || self.close_armed
    }

    /// 折り畳んでいる間の表示。押すと展開する。
    fn bar(&self) -> Element<'_, Message> {
        let has_notice = self.has_notice();
        let label = if has_notice {
            "要確認 — クリックで開く"
        } else {
            "クリックで開く"
        };

        mouse_area(
            container(text(label).size(13))
                .center(Fill)
                .style(move |_| notice_style(has_notice)),
        )
        .on_press(Message::Expand)
        .interaction(mouse::Interaction::Pointer)
        .into()
    }

    pub fn view(&self) -> Element<'_, Message> {
        if self.collapsed {
            return self.bar();
        }

        let toolbar = row![
            space::horizontal(),
            button(text("最前面に固定").size(12))
                .padding([2, 8])
                .style(if self.pinned {
                    button::primary
                } else {
                    button::text
                })
                .on_press(Message::TogglePin),
            button(text("折り畳む").size(12))
                .padding([2, 8])
                .style(button::text)
                .on_press(Message::ToggleCollapse),
        ]
        .spacing(4)
        .padding([4, 8]);

        let mut editor = iced::widget::text_editor(&self.content)
            .id(EDITOR_ID)
            .on_action(Message::Edit)
            .key_binding(key_binding)
            .size(f32::from(self.font_size))
            .padding(12)
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
            .height(Fill)
            .style(|theme, status| text_editor::Style {
                border: iced::Border::default(),
                ..text_editor::default(theme, status)
            });
        if let Some(font) = self.editor_font {
            editor = editor.font(font);
        }

        // ピッカーや通知の有無でウィジェット構造を変えない（エディタのフォーカスを保つ）
        let mut body = stack![editor];
        if let Some((kind, selected)) = self.picker {
            body = body.push(ui::picker::view(self.picker_items(kind), selected));
        }

        // 過去のメモへ書いていることに気づけるよう、開いている間は上部に出し続ける
        let mut past = column![];
        if self.viewing_past {
            past = past.push(
                text(format!(
                    "{} のメモを開いています（編集できます）。{}+E で今日へ戻れます。",
                    date::label(self.active_date),
                    if cfg!(target_os = "macos") {
                        "Cmd"
                    } else {
                        "Ctrl"
                    }
                ))
                .size(13),
            );
        }
        let viewing_past = self.viewing_past;

        let mut notices = column![].spacing(4);
        let messages = [&self.config_error, &self.load_error, &self.save_error];
        for message in messages.into_iter().flatten() {
            notices = notices.push(text(message.as_str()).size(13));
        }
        if self.close_armed {
            notices = notices.push(
                text(
                    "未保存の内容があります。もう一度閉じると、未保存の内容を破棄して終了します。",
                )
                .size(13),
            );
        }
        let has_notice = self.has_notice();

        column![
            toolbar,
            container(past)
                .width(Fill)
                .padding(if viewing_past { [6, 12] } else { [0, 0] })
                .style(move |_| {
                    if viewing_past {
                        container::Style {
                            background: Some(Color::from_rgb8(0xFB, 0xF1, 0xD0).into()),
                            text_color: Some(Color::from_rgb8(0x6B, 0x50, 0x0A)),
                            ..container::Style::default()
                        }
                    } else {
                        container::Style::default()
                    }
                }),
            body,
            container(notices)
                .width(Fill)
                .padding(if has_notice { [8, 12] } else { [0, 0] })
                .style(move |_| notice_style(has_notice))
        ]
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            event::listen_with(runtime_event),
            window::close_requests().map(Message::CloseRequested),
            saver::subscription().map(Message::Saver),
        ])
    }
}

/// 通知がある時の背景と文字色。
fn notice_style(has_notice: bool) -> container::Style {
    if has_notice {
        container::Style {
            background: Some(Color::from_rgb8(0xFD, 0xEC, 0xEC).into()),
            text_color: Some(Color::from_rgb8(0x9B, 0x1C, 0x1C)),
            ..container::Style::default()
        }
    } else {
        container::Style::default()
    }
}

/// いまの内寸を調べてから折り畳む。フルスクリーン中は折り畳まない。
fn request_collapse() -> Task<Message> {
    window::latest().and_then(|id| {
        window::mode(id).then(move |mode| {
            if mode == window::Mode::Fullscreen {
                Task::none()
            } else {
                window::size(id).map(Message::Collapse)
            }
        })
    })
}

/// エディタにフォーカスがなくても効くショートカット（最前面固定・折り畳み・ズーム）。
fn app_shortcut(
    key: &Key,
    physical: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
) -> Option<Message> {
    if !modifiers.command() {
        return None;
    }
    let pressed = key.to_latin(physical)?.to_ascii_lowercase();
    if modifiers.shift() {
        match pressed {
            't' => return Some(Message::TogglePin),
            'm' => return Some(Message::ToggleCollapse),
            _ => {}
        }
    }
    // Windows の AltGr は Ctrl+Alt として届く。記号の入力をズームと取り違えない
    if modifiers.alt() {
        return None;
    }
    match pressed {
        // `+` は Shift なしの同じキー（US 配列の `=`、JIS 配列の `;`）でも受ける
        '+' | '=' | ';' => Some(Message::Zoom(1)),
        '-' => Some(Message::Zoom(-1)),
        '0' => Some(Message::ZoomReset),
        _ => None,
    }
}

fn runtime_event(event: Event, status: event::Status, _window: window::Id) -> Option<Message> {
    match event {
        Event::Window(window::Event::Opened { .. }) => Some(Message::WindowOpened),
        Event::Window(window::Event::Focused) => Some(Message::WindowFocused),
        Event::Window(window::Event::Unfocused) => Some(Message::WindowUnfocused),
        Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
            Some(Message::ModifiersChanged(modifiers))
        }
        Event::Mouse(mouse::Event::WheelScrolled { delta }) => Some(Message::WheelScrolled(delta)),
        Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            ..
        }) if status == event::Status::Ignored => {
            Some(Message::KeyPressed(key, physical_key, modifiers))
        }
        _ => None,
    }
}

/// 貼り付け・IME確定に含まれる CR を LF へ揃える（保存ファイルを LF に保つ）。
fn normalize_paste(action: text_editor::Action) -> text_editor::Action {
    match action {
        text_editor::Action::Edit(text_editor::Edit::Paste(text)) if text.contains('\r') => {
            text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(
                storage::normalize_newlines(&text),
            )))
        }
        action => action,
    }
}

fn key_binding(press: KeyPress) -> Option<Binding<Message>> {
    if !matches!(press.status, Status::Focused { .. }) {
        return None;
    }

    let modifiers = press.modifiers;
    if let Some(message) = app_shortcut(&press.key, press.physical_key, modifiers) {
        return Some(Binding::Custom(message));
    }
    if modifiers.command() {
        let custom = match (press.key.to_latin(press.physical_key), modifiers.shift()) {
            (Some('k'), false) => Some(Message::OpenPicker(PickerKind::Entry)),
            (Some('l'), false) => Some(Message::OpenPicker(PickerKind::Prefix)),
            (Some('e'), false) => Some(Message::OpenPicker(PickerKind::Day)),
            (Some('c'), true) => Some(Message::CopyAll),
            (Some('z'), false) => Some(Message::Undo),
            (Some('z'), true) => Some(Message::Redo),
            (Some('y'), false) if !cfg!(target_os = "macos") => Some(Message::Redo),
            (Some('s'), false) => Some(Message::Flush),
            _ => None,
        };
        if let Some(message) = custom {
            return Some(Binding::Custom(message));
        }
    }

    // Esc でエディタのフォーカスを外さない（常にそのまま書けるようにする）
    if press.key == Key::Named(key::Named::Escape) {
        return None;
    }

    Binding::from_key_press(press)
}
