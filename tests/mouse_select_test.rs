//! マウス選択のラッパーを、ウィンドウなしの UI にマウスイベントを流して確かめる。

use std::time::Instant;

use iced::advanced::clipboard;
use iced::keyboard::{self, Key, Location, Modifiers, key};
use iced::widget::text_editor::{Action, Content, Position};
use iced::{Element, Event, Fill, Font, Pixels, Point, Rectangle, Size, Theme, mouse, window};
use iced_runtime::user_interface::{Cache, UserInterface};

use tsuratsura::ui::mouse_select::{
    AUTOSCROLL_INTERVAL, autoscroll_lines, drag_point, mouse_select,
};

type Renderer = iced_tiny_skia::Renderer;

const PADDING: f32 = 12.0;
/// エディタがウィンドウ全体を占める。本文領域は (12, 12)〜(388, 288)。
const WINDOW: Size = Size::new(400.0, 300.0);
/// 文字サイズ 16・行の高さ 1.3 倍で、本文領域には 13 行ほど見える。
const VISIBLE_LINES: usize = 13;
const LINE_HEIGHT: f32 = 16.0 * 1.3;

/// スクロールしていない時に、`line` 行目の中ほどに当たる y 座標。
fn line_y(line: usize) -> f32 {
    PADDING + LINE_HEIGHT * line as f32 + LINE_HEIGHT / 2.0
}

struct Harness {
    content: Content<Renderer>,
    renderer: Renderer,
    cache: Cache,
    cursor: Point,
    now: Instant,
}

impl Harness {
    fn new(lines: usize) -> Self {
        let text = (0..lines)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut harness = Self {
            content: Content::with_text(&text),
            renderer: Renderer::new(Font::default(), Pixels(16.0)),
            cache: Cache::new(),
            cursor: Point::new(200.0, 150.0),
            now: Instant::now(),
        };
        harness.frame();
        harness
    }

    /// 末尾までスクロールした状態で始める。
    fn at_end(lines: usize) -> Self {
        let mut harness = Self::new(lines);
        harness
            .content
            .perform(Action::Move(iced::widget::text_editor::Motion::DocumentEnd));
        harness.frame();
        harness
    }

    /// 実行時と同じく、毎回レイアウトし直した UI へイベントを1つ流し、出たアクションを本文へ適用する。
    fn send(&mut self, event: Event) {
        let editor = iced::widget::text_editor(&self.content)
            .on_action(|action| action)
            .padding(PADDING)
            .size(16)
            .height(Fill);
        let root: Element<'_, Action, Theme, Renderer> =
            mouse_select(editor, PADDING, |action| action).into();

        let mut ui = UserInterface::build(
            root,
            WINDOW,
            std::mem::take(&mut self.cache),
            &mut self.renderer,
        );
        let mut actions = Vec::new();
        let _ = ui.update(
            &[event],
            mouse::Cursor::Available(self.cursor),
            &mut self.renderer,
            &mut clipboard::Null,
            &mut actions,
        );
        self.cache = ui.into_cache();

        for action in actions {
            self.content.perform(action);
        }
    }

    fn move_to(&mut self, x: f32, y: f32) {
        self.cursor = Point::new(x, y);
        self.send(Event::Mouse(mouse::Event::CursorMoved {
            position: self.cursor,
        }));
    }

    fn press(&mut self, x: f32, y: f32) {
        self.move_to(x, y);
        self.send(Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        )));
    }

    fn release(&mut self) {
        self.send(Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        )));
    }

    /// 自動スクロールの間隔ぶん時間を進めて再描画する。
    fn frame(&mut self) {
        self.now += AUTOSCROLL_INTERVAL;
        self.send(Event::Window(window::Event::RedrawRequested(self.now)));
    }

    fn frames(&mut self, count: usize) {
        for _ in 0..count {
            self.frame();
        }
    }

    fn modifiers(&mut self, modifiers: Modifiers) {
        self.send(Event::Keyboard(keyboard::Event::ModifiersChanged(
            modifiers,
        )));
    }

    /// Shift を押したまま押下する（離すのは呼び出し側）。
    fn shift_press(&mut self, x: f32, y: f32) {
        self.modifiers(Modifiers::SHIFT);
        self.press(x, y);
    }

    fn shift_click(&mut self, x: f32, y: f32) {
        self.shift_press(x, y);
        self.release();
        self.modifiers(Modifiers::empty());
    }

    fn wheel(&mut self, lines: f32) {
        self.send(Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: lines },
        }));
        self.frame();
    }

    fn key(&mut self, key: Key, physical: key::Code, modifiers: Modifiers, text: Option<&str>) {
        self.send(Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: key::Physical::Code(physical),
            location: Location::Standard,
            modifiers,
            text: text.map(Into::into),
            repeat: false,
        }));
        self.frame();
    }

    /// 表示中の先頭行。本文の右上をクリックして調べるため、選択は解除される。
    fn top_line(&mut self) -> usize {
        self.press(WINDOW.width - 20.0, PADDING + 1.0);
        self.release();
        self.cursor_line()
    }

    fn cursor_line(&self) -> usize {
        self.content.cursor().position.line
    }

    fn anchor(&self) -> Option<Position> {
        self.content.cursor().selection
    }
}

#[test]
fn dragging_below_the_editor_scrolls_and_extends_the_selection() {
    let mut h = Harness::new(200);
    h.press(12.0, 14.0);
    h.move_to(200.0, 350.0);
    let at_edge = h.cursor_line();
    assert!(
        (VISIBLE_LINES - 2..=VISIBLE_LINES).contains(&at_edge),
        "外へ出た時点では見えている最終行まで: {at_edge}"
    );

    h.frames(10);

    assert!(h.cursor_line() > at_edge + 20, "{}", h.cursor_line());
    assert_eq!(h.anchor(), Some(Position { line: 0, column: 0 }));
    assert!(
        h.content
            .selection()
            .unwrap()
            .starts_with("line 0\nline 1\n")
    );
}

#[test]
fn dragging_above_the_editor_scrolls_up() {
    let mut h = Harness::at_end(200);
    h.press(14.0, 280.0);
    let start = h.cursor_line();
    assert!(start > 180, "{start}");

    h.move_to(200.0, -40.0);
    h.frames(10);

    assert!(
        h.cursor_line() + VISIBLE_LINES + 20 < start,
        "{}",
        h.cursor_line()
    );
    assert_eq!(h.anchor().map(|p| p.line), Some(start));
}

#[test]
fn bottom_padding_alone_is_enough_to_autoscroll() {
    // 最大化などでウィンドウの外へ出せない時も、下の余白に入れれば少しずつ進む
    let mut h = Harness::new(200);
    h.press(12.0, 14.0);
    h.move_to(200.0, 295.0);
    let at_edge = h.cursor_line();

    h.frames(5);

    assert!(
        h.cursor_line() >= at_edge + 4,
        "{} / {at_edge}",
        h.cursor_line()
    );
}

#[test]
fn autoscroll_stops_when_the_cursor_comes_back() {
    let mut h = Harness::new(200);
    h.press(12.0, 14.0);
    h.move_to(200.0, 350.0);
    h.frames(3);
    h.move_to(200.0, 150.0);
    let line = h.cursor_line();

    h.frames(5);

    assert_eq!(h.cursor_line(), line);
}

#[test]
fn release_stops_autoscroll() {
    let mut h = Harness::new(200);
    h.press(12.0, 14.0);
    h.move_to(200.0, 350.0);
    h.frames(3);
    h.release();
    let line = h.cursor_line();

    h.frames(5);
    h.move_to(200.0, 360.0);
    h.frames(5);

    assert_eq!(h.cursor_line(), line);
    assert!(h.anchor().is_some(), "離した後も選択は残る");
}

#[test]
fn window_unfocus_ends_the_drag() {
    let mut h = Harness::new(200);
    h.press(12.0, 14.0);
    h.move_to(200.0, 350.0);
    h.frames(2);
    h.send(Event::Window(window::Event::Unfocused));
    let line = h.cursor_line();

    h.frames(5);
    h.move_to(200.0, 100.0);

    assert_eq!(h.cursor_line(), line);
}

#[test]
fn moving_outside_without_dragging_changes_nothing() {
    let mut h = Harness::new(200);
    h.move_to(200.0, 350.0);
    h.frames(5);

    assert_eq!(h.cursor_line(), 0);
    assert_eq!(h.anchor(), None);
}

#[test]
fn wheel_while_dragging_moves_the_selection_end() {
    let mut h = Harness::new(200);
    h.press(12.0, 14.0);
    h.move_to(200.0, 100.0);
    let before = h.cursor_line();

    h.send(Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 },
    }));
    h.frame();

    assert_eq!(h.cursor_line(), before + 8);
    assert_eq!(h.anchor(), Some(Position { line: 0, column: 0 }));
}

#[test]
fn double_click_drag_keeps_the_word_selection() {
    // エディタと同じく、ドラッグで伸ばすのはシングルクリックの時だけ
    let mut h = Harness::new(200);
    h.move_to(30.0, 14.0);
    // ダブルクリックの判定は実時間（300ms 以内）に依存する。並列実行で遅れた時は押し直す
    let mut selected = None;
    for _ in 0..10 {
        h.send(Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        )));
        if h.content.selection().as_deref() == Some("line") {
            selected = h.content.selection();
            break;
        }
        h.release();
    }
    assert_eq!(
        selected.as_deref(),
        Some("line"),
        "ダブルクリックとして認識されない"
    );

    h.move_to(200.0, 350.0);
    h.frames(3);

    assert_eq!(h.content.selection(), selected);
}

const TEXT: Rectangle = Rectangle {
    x: 12.0,
    y: 12.0,
    width: 376.0,
    height: 276.0,
};

#[test]
fn drag_point_is_clamped_into_the_text_area() {
    assert_eq!(
        drag_point(Point::new(100.0, 50.0), TEXT),
        Point::new(88.0, 38.0)
    );
    assert_eq!(
        drag_point(Point::new(-30.0, -30.0), TEXT),
        Point::new(0.0, 0.0)
    );
    assert_eq!(
        drag_point(Point::new(500.0, 500.0), TEXT),
        Point::new(376.0, 275.0)
    );
}

#[test]
fn autoscroll_speed_grows_with_distance() {
    assert_eq!(autoscroll_lines(Point::new(100.0, 12.0), TEXT), 0);
    assert_eq!(autoscroll_lines(Point::new(100.0, 288.0), TEXT), 0);
    assert_eq!(autoscroll_lines(Point::new(100.0, 11.0), TEXT), -1);
    assert_eq!(autoscroll_lines(Point::new(100.0, 289.0), TEXT), 1);
    assert_eq!(autoscroll_lines(Point::new(100.0, 288.0 + 50.0), TEXT), 3);
    assert_eq!(autoscroll_lines(Point::new(100.0, -1000.0), TEXT), -8);
}

#[test]
fn shift_click_extends_the_selection_from_the_caret() {
    let mut h = Harness::new(200);
    h.press(12.0, line_y(0));
    h.release();
    // ホイールで 20 行下へスクロールしてから、見えている 5 行目を Shift+クリックする
    h.wheel(-5.0);
    h.shift_click(300.0, line_y(5));

    assert_eq!(h.anchor(), Some(Position { line: 0, column: 0 }));
    assert_eq!(h.cursor_line(), 25);
    let selected = h.content.selection().unwrap();
    assert!(selected.starts_with("line 0\n"), "{selected}");
    assert!(selected.ends_with("\nline 25"), "{selected}");
}

#[test]
fn shift_click_keeps_the_anchor_of_an_existing_selection() {
    let mut h = Harness::new(200);
    h.press(12.0, line_y(5));
    h.move_to(12.0, line_y(8));
    h.release();
    assert_eq!(h.anchor(), Some(Position { line: 5, column: 0 }));

    // 起点より上を Shift+クリックすると、起点はそのままで向きが変わる
    h.shift_click(12.0, line_y(2));

    assert_eq!(h.anchor(), Some(Position { line: 5, column: 0 }));
    assert_eq!(h.content.cursor().position, Position { line: 2, column: 0 });
}

#[test]
fn shift_click_focuses_the_editor() {
    let mut h = Harness::new(200);
    h.shift_click(300.0, line_y(3));
    assert_eq!(h.anchor(), Some(Position { line: 0, column: 0 }));

    // フォーカスがあれば、入力で選択範囲が置き換わる
    h.key(
        Key::Character("x".into()),
        key::Code::KeyX,
        Modifiers::empty(),
        Some("x"),
    );

    assert_eq!(h.content.line(0).unwrap().text, "x");
    assert_eq!(h.content.line(1).unwrap().text, "line 4");
}

#[test]
fn click_without_shift_after_a_shift_click_starts_over() {
    let mut h = Harness::new(200);
    h.shift_click(300.0, line_y(3));
    h.press(12.0, line_y(6));
    h.release();

    assert_eq!(h.anchor(), None);
    assert_eq!(h.cursor_line(), 6);
}

#[test]
fn dragging_after_a_shift_click_keeps_extending_and_autoscrolls() {
    let mut h = Harness::new(200);
    h.press(12.0, line_y(0));
    h.release();
    h.shift_press(12.0, line_y(5));
    h.move_to(200.0, 350.0);
    h.frames(10);
    h.release();

    assert_eq!(h.anchor(), Some(Position { line: 0, column: 0 }));
    assert!(h.cursor_line() > VISIBLE_LINES + 20, "{}", h.cursor_line());
}

#[test]
fn shift_arrow_extends_the_selection_past_the_visible_area() {
    // キー操作はエディタ自身が扱う。表示範囲の外まで選択を伸ばせ、表示も付いてくることを固定する
    let mut h = Harness::new(200);
    h.press(12.0, line_y(0));
    h.release();
    for _ in 0..30 {
        h.key(
            Key::Named(key::Named::ArrowDown),
            key::Code::ArrowDown,
            Modifiers::SHIFT,
            None,
        );
    }

    assert_eq!(h.anchor(), Some(Position { line: 0, column: 0 }));
    assert_eq!(h.cursor_line(), 30);
    let top = h.top_line();
    assert!((30 - VISIBLE_LINES..30).contains(&top), "{top}");
}
