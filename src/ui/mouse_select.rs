//! TextEditor のマウス選択を補うラッパー。
//!
//! Iced 0.14 の TextEditor は、ドラッグ中のカーソルが本文の外へ出ると選択を伸ばさず、
//! 本文もスクロールしない。Shift+クリックでの範囲選択もない。このラッパーは
//! - ドラッグを引き受けて、カーソル位置を本文の範囲へ収めて選択の端を動かし、
//! - 本文の上下端（余白を含む）より外では、離れた距離に応じて自動スクロールし、
//! - ドラッグ中のホイールでも選択の端をカーソルへ追従させ、
//! - Shift+クリックで、選択の起点を保ったまま端をクリック位置へ伸ばす。

use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::widget::text_editor::Action;
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, keyboard, window};

/// 自動スクロールの間隔。
pub const AUTOSCROLL_INTERVAL: Duration = Duration::from_millis(50);

/// 本文の外へこの距離（px）離れるごとに、1回の自動スクロール量を1行増やす。
const AUTOSCROLL_PIXELS_PER_LINE: f32 = 24.0;

/// 1回の自動スクロールの最大行数。
const AUTOSCROLL_MAX_LINES: i32 = 8;

/// `editor` のマウス選択を補う。`padding` は `editor` に指定した余白、
/// `on_action` は `editor` の `on_action` と同じものを渡す。
pub fn mouse_select<'a, Message, Theme, Renderer>(
    editor: impl Into<Element<'a, Message, Theme, Renderer>>,
    padding: f32,
    on_action: impl Fn(Action) -> Message + 'a,
) -> MouseSelect<'a, Message, Theme, Renderer> {
    MouseSelect {
        content: editor.into(),
        padding,
        on_action: Box::new(on_action),
    }
}

/// ドラッグ中のカーソル位置（ウィンドウ座標）を、本文領域 `text` 内の座標へ収める。
pub fn drag_point(cursor: Point, text: Rectangle) -> Point {
    Point::new(
        (cursor.x - text.x).clamp(0.0, text.width.max(0.0)),
        // 下端ちょうどは最終行の外になるため、わずかに内側へ寄せる
        (cursor.y - text.y).clamp(0.0, (text.height - 1.0).max(0.0)),
    )
}

/// 1回の自動スクロールの行数。正なら下、負なら上、本文領域 `text` の上下端の内側なら 0。
pub fn autoscroll_lines(cursor: Point, text: Rectangle) -> i32 {
    let over = if cursor.y < text.y {
        cursor.y - text.y
    } else if cursor.y > text.y + text.height {
        cursor.y - (text.y + text.height)
    } else {
        return 0;
    };
    let lines = (1 + (over.abs() / AUTOSCROLL_PIXELS_PER_LINE) as i32).min(AUTOSCROLL_MAX_LINES);
    if over < 0.0 { -lines } else { lines }
}

pub struct MouseSelect<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    padding: f32,
    on_action: Box<dyn Fn(Action) -> Message + 'a>,
}

#[derive(Default)]
struct State {
    drag: Option<Drag>,
    /// エディタが受けた直前のクリック。ダブル・トリプルクリックの判定をエディタと揃える。
    last_click: Option<mouse::Click>,
    /// マウスイベントは修飾キーを持たないため、Shift+クリックの判定用に覚えておく。
    modifiers: keyboard::Modifiers,
}

struct Drag {
    /// 最後に分かったカーソル位置（ウィンドウ座標）。ウィンドウ外でも更新される。
    cursor: Point,
    /// 次に自動スクロールしてよい時刻。本文の範囲内にいる間は `None`。
    next_scroll: Option<Instant>,
    /// 本文がスクロールした。次の再描画（レイアウト更新後）で選択の端をカーソルへ合わせ直す。
    resync: bool,
}

impl Drag {
    fn new(cursor: Point) -> Self {
        Self {
            cursor,
            next_scroll: None,
            resync: false,
        }
    }
}

/// エディタにフォーカスを与える（Shift+クリックはエディタへ渡さないため）。
struct FocusEditor;

impl Operation for FocusEditor {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if !state.is_focused() {
            state.focus();
        }
    }
}

impl<Message, Theme, Renderer> MouseSelect<'_, Message, Theme, Renderer> {
    fn text_bounds(&self, layout: Layout<'_>) -> Rectangle {
        layout.bounds().shrink(self.padding)
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for MouseSelect<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let text = self.text_bounds(layout);
        let state = tree.state.downcast_mut::<State>();
        let editor = &mut tree.children[0];

        match event {
            // ドラッグ中のカーソル移動はここで引き受ける（エディタは本文の外の移動を無視するため）
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(drag) = &mut state.drag {
                    if let Some(position) = cursor.position() {
                        drag.cursor = position;
                    }
                    shell.publish((self.on_action)(Action::Drag(drag_point(
                        drag.cursor,
                        text,
                    ))));
                    if autoscroll_lines(drag.cursor, text) == 0 {
                        drag.next_scroll = None;
                    } else {
                        shell.request_redraw();
                    }
                    return;
                }
            }
            // Shift+クリックはエディタへ渡さない（渡すと選択が解除される）。
            // 選択の起点（選択がなければ今のカーソル位置）を保ったまま、端をクリック位置へ動かす
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if state.modifiers.shift() && !shell.is_event_captured() =>
            {
                if let Some(position) = cursor.position_over(layout.bounds()) {
                    self.content.as_widget_mut().operate(
                        editor,
                        layout,
                        renderer,
                        &mut FocusEditor,
                    );
                    shell.publish((self.on_action)(Action::Drag(drag_point(position, text))));
                    // 続けてドラッグすれば、そのまま選択の端を動かせる
                    state.drag = Some(Drag::new(position));
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
            }
            _ => {}
        }

        let was_captured = shell.is_event_captured();
        self.content.as_widget_mut().update(
            editor, event, layout, cursor, renderer, clipboard, shell, viewport,
        );

        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                state.drag = None;
                // エディタがクリックとして受けた時だけ数える
                let Some(position) = cursor.position() else {
                    return;
                };
                if was_captured || !shell.is_event_captured() {
                    return;
                }
                let click = mouse::Click::new(
                    position - Vector::new(text.x, text.y),
                    mouse::Button::Left,
                    state.last_click,
                );
                state.last_click = Some(click);
                // エディタと同じく、ドラッグで選択を伸ばすのはシングルクリックの時だけ
                if click.kind() == mouse::click::Kind::Single {
                    state.drag = Some(Drag::new(position));
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag = None;
            }
            // ボタンを離したことが届かない場合に備え、ウィンドウを離れたらドラッグを終える
            Event::Window(window::Event::Unfocused) => {
                if state.drag.take().is_some() {
                    self.content.as_widget_mut().update(
                        editor,
                        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                        layout,
                        cursor,
                        renderer,
                        clipboard,
                        shell,
                        viewport,
                    );
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { .. }) => {
                if let Some(drag) = &mut state.drag {
                    drag.resync = true;
                    shell.request_redraw();
                }
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                if let Some(drag) = &mut state.drag {
                    redraw_tick(drag, *now, text, &self.on_action, shell);
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

/// 再描画のたびに、スクロール後の選択の合わせ直しと自動スクロールを行う。
fn redraw_tick<Message>(
    drag: &mut Drag,
    now: Instant,
    text: Rectangle,
    on_action: &dyn Fn(Action) -> Message,
    shell: &mut Shell<'_, Message>,
) {
    // スクロール直後は新しく見えた行のレイアウトがまだないため、再描画まで待ってから合わせる
    if drag.resync {
        drag.resync = false;
        shell.publish(on_action(Action::Drag(drag_point(drag.cursor, text))));
    }

    let lines = autoscroll_lines(drag.cursor, text);
    if lines == 0 {
        drag.next_scroll = None;
        return;
    }
    match drag.next_scroll {
        Some(at) if now < at => shell.request_redraw_at(at),
        _ => {
            shell.publish(on_action(Action::Scroll { lines }));
            drag.next_scroll = Some(now + AUTOSCROLL_INTERVAL);
            drag.resync = true;
            shell.request_redraw();
        }
    }
}

impl<'a, Message, Theme, Renderer> From<MouseSelect<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(widget: MouseSelect<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}
