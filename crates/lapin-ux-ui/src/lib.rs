// SPDX-License-Identifier: GPL-3.0-only
//! Windows-style widget styles shared by the Lapin apps.
//!
//! The theme keeps COSMIC's largest corner radius fully round, because that
//! radius draws toggle switches, slider thumbs and progress bars, which are
//! round in Windows 11. libcosmic also uses it for ordinary push buttons,
//! which in Windows have small 4 px corners, so the Lapin apps give their
//! buttons these classes instead.

use cosmic::theme::Button;
use cosmic::widget::button::{Catalog, Style};

/// Corner radius of Windows 11 push buttons.
pub const BUTTON_RADIUS: f32 = 4.0;

fn square(mut s: Style) -> Style {
    s.border_radius = BUTTON_RADIUS.into();
    s
}

/// `kind` drawn with 4 px corners.
pub fn win(kind: fn() -> Button) -> Button {
    Button::Custom {
        active: Box::new(move |f, t| square(t.active(f, false, &kind()))),
        disabled: Box::new(move |t| square(t.disabled(&kind()))),
        hovered: Box::new(move |f, t| square(t.hovered(f, false, &kind()))),
        pressed: Box::new(move |f, t| square(t.pressed(f, false, &kind()))),
    }
}

pub fn standard() -> Button {
    win(|| Button::Standard)
}
pub fn suggested() -> Button {
    win(|| Button::Suggested)
}
pub fn destructive() -> Button {
    win(|| Button::Destructive)
}
pub fn text() -> Button {
    win(|| Button::Text)
}
pub fn icon() -> Button {
    win(|| Button::Icon)
}

/// Drop-in replacements for `cosmic::widget::button::{standard, suggested,
/// destructive, text, icon}` with Windows' 4 px corners.
pub mod button {
    use std::borrow::Cow;

    use cosmic::widget::button::{self, IconButton, TextButton};
    use cosmic::widget::icon::Handle;

    pub fn standard<'a, M>(label: impl Into<Cow<'a, str>>) -> TextButton<'a, M> {
        button::standard(label).class(super::standard())
    }
    pub fn suggested<'a, M>(label: impl Into<Cow<'a, str>>) -> TextButton<'a, M> {
        button::suggested(label).class(super::suggested())
    }
    pub fn destructive<'a, M>(label: impl Into<Cow<'a, str>>) -> TextButton<'a, M> {
        button::destructive(label).class(super::destructive())
    }
    pub fn text<'a, M>(label: impl Into<Cow<'a, str>>) -> TextButton<'a, M> {
        button::text(label).class(super::text())
    }
    pub fn icon<'a, M>(handle: impl Into<Handle>) -> IconButton<'a, M> {
        button::icon(handle).class(super::icon())
    }

    /// A fixed-width push button with its label centred both ways (libcosmic
    /// text buttons keep the label at the left when given a width). Windows
    /// dialog buttons look like this.
    pub fn sized<'a, M: Clone + 'a>(
        label: impl Into<Cow<'a, str>> + 'a,
        width: f32,
        class: cosmic::theme::Button,
    ) -> cosmic::widget::Button<'a, M> {
        use cosmic::iced::Length;
        button::custom(
            cosmic::widget::container(cosmic::widget::text::body(label))
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(Length::Fixed(width))
        .height(Length::Fixed(32.0))
        .padding(0)
        .class(class)
    }
}

/// Sliders with a round thumb. libcosmic draws the thumb with the theme's
/// medium radius, which Lapin keeps at 8 px for windows and panels, so the
/// thumb would otherwise come out as a rounded square.
pub fn slider() -> cosmic::theme::style::iced::Slider {
    use cosmic::iced::widget::slider::{Catalog, HandleShape, Status};
    use cosmic::theme::style::iced::Slider;
    use std::rc::Rc;
    fn round(t: &cosmic::Theme, status: Status) -> cosmic::iced::widget::slider::Style {
        let mut s = t.style(&Slider::Standard, status);
        if let HandleShape::Rectangle { width, height, .. } = s.handle.shape {
            s.handle.shape = HandleShape::Rectangle { width, height, border_radius: (f32::from(width.max(height)) / 2.0).into() };
        }
        s
    }
    Slider::Custom {
        active: Rc::new(|t| round(t, Status::Active)),
        hovered: Rc::new(|t| round(t, Status::Hovered)),
        dragging: Rc::new(|t| round(t, Status::Dragged)),
    }
}

/// A page in a sidebar (Task Manager, Settings…): plain text, a soft
/// highlight on hover, and a filled background when it's the open page.
pub fn nav_item(selected: bool) -> Button {
    fn plain(mut s: Style, t: &cosmic::Theme) -> Style {
        let on: cosmic::iced::Color = t.cosmic().on_bg_color().into();
        s.text_color = Some(on);
        s.icon_color = Some(on);
        square(s)
    }
    let kind = move || if selected { Button::Standard } else { Button::Text };
    Button::Custom {
        active: Box::new(move |f, t| plain(t.active(f, false, &kind()), t)),
        disabled: Box::new(move |t| plain(t.disabled(&kind()), t)),
        hovered: Box::new(move |f, t| plain(t.hovered(f, false, &kind()), t)),
        pressed: Box::new(move |f, t| plain(t.pressed(f, false, &kind()), t)),
    }
}

/// Windows 11's title: the app's icon and name at the left of the title
/// bar (the same look the libcosmic patch gives COSMIC's own apps).
pub fn title_bar<'a, M: 'static>(icon_name: &'a str, title: &'a str) -> cosmic::Element<'a, M> {
    cosmic::widget::row::with_capacity(2)
        .spacing(10)
        .padding([0, 0, 0, 6])
        .align_y(cosmic::iced::Alignment::Center)
        .push(cosmic::widget::icon::from_name(icon_name).size(16))
        .push(cosmic::widget::text::body(title))
        .into()
}

/// Screen reader helpers.
pub mod a11y {
    use cosmic::iced::core::widget::{tree, Operation, Tree};
    use cosmic::iced::core::{layout, mouse, overlay, renderer, Clipboard, Event, Layout, Length, Rectangle, Shell, Size, Vector};
    use cosmic::{Element, Renderer, Theme};

    /// Draws `content` as usual but leaves it out of what screen readers see:
    /// decoration that repeats something already there, such as a text's
    /// drop shadow (a second copy of the text underneath it).
    pub fn hidden<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        Element::new(Wrap { kind: Kind::Hidden, content: content.into() })
    }

    /// Gives `content` a name for screen readers, as one group: what a
    /// panel or pop-up is called ("Taskbar", "Start", "Quick settings").
    /// Wayland panels have no window title for a screen reader to read.
    pub fn group<'a, Message: 'a>(name: impl Into<String>, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        Element::new(Wrap { kind: Kind::Group(name.into(), cosmic::widget::Id::unique()), content: content.into() })
    }

    enum Kind {
        Hidden,
        Group(String, cosmic::widget::Id),
    }

    struct Wrap<'a, Message> {
        kind: Kind,
        content: Element<'a, Message>,
    }

    // Everything goes to the content, tag and state included, so the
    // widget tree is exactly the content's (see #29 for what a wrapper with
    // a different tag but the content's state does). Only `a11y_nodes`
    // differs.
    impl<'a, Message> cosmic::iced::core::Widget<Message, Theme, Renderer> for Wrap<'a, Message> {
        fn tag(&self) -> tree::Tag {
            self.content.as_widget().tag()
        }
        fn state(&self) -> tree::State {
            self.content.as_widget().state()
        }
        fn children(&self) -> Vec<Tree> {
            self.content.as_widget().children()
        }
        fn diff(&mut self, tree: &mut Tree) {
            self.content.as_widget_mut().diff(tree);
        }
        fn size(&self) -> Size<Length> {
            self.content.as_widget().size()
        }
        fn size_hint(&self) -> Size<Length> {
            self.content.as_widget().size_hint()
        }
        fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
            self.content.as_widget_mut().layout(tree, renderer, limits)
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
            self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
            self.content.as_widget_mut().operate(tree, layout, renderer, operation);
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
            self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
        }
        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'b>,
            renderer: &Renderer,
            viewport: &Rectangle,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
            self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation)
        }
        fn a11y_nodes(&self, layout: Layout<'_>, state: &Tree, cursor: mouse::Cursor) -> iced_accessibility::A11yTree {
            use iced_accessibility::accesskit::{Node, Rect, Role};
            use iced_accessibility::{A11yNode, A11yTree};
            match &self.kind {
                Kind::Hidden => A11yTree::default(),
                Kind::Group(name, id) => {
                    let b = layout.bounds();
                    let mut node = Node::new(Role::Group);
                    node.set_label(name.clone());
                    node.set_bounds(Rect {
                        x0: b.x as f64,
                        y0: b.y as f64,
                        x1: (b.x + b.width) as f64,
                        y1: (b.y + b.height) as f64,
                    });
                    let inner = self.content.as_widget().a11y_nodes(layout, state, cursor);
                    A11yTree::node_with_child_tree(A11yNode::new(node, id.clone()), inner)
                }
            }
        }
    }
}
