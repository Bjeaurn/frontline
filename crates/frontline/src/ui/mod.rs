//! Shared look of every menu screen (CoD4-ish): fonts, palette and widgets. Client/Host only.
//!
//! Build screens from [`title`], [`label`], [`button`], [`panel`], [`backdrop`], [`slider`]
//! and [`text_field`]; buttons get hover/press styling automatically. React to a click with an
//! observer on the button entity:
//! `commands.spawn(button(&fonts, "Host Game")).observe(|_: On<Activate>, ...| { ... })`
//! (`bevy::ui_widgets::Activate`; also fires on Enter/Space when the button has focus).

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{EditableText, TextCursorStyle};
use bevy::ui::Pressed;
use bevy::ui_widgets::{
    Button, Slider, SliderRange, SliderThumb, SliderValue, TextInput, observe, slider_self_update,
};

pub const BACKDROP: Color = Color::srgba(0.02, 0.03, 0.02, 0.72);
pub const PANEL: Color = Color::srgba(0.07, 0.08, 0.06, 0.88);
pub const PANEL_BORDER: Color = Color::srgba(0.55, 0.52, 0.38, 0.5);
pub const TEXT: Color = Color::srgb(0.90, 0.88, 0.80);
pub const TEXT_DIM: Color = Color::srgb(0.60, 0.58, 0.52);
/// Olive/khaki accent for highlights and selection.
pub const ACCENT: Color = Color::srgb(0.78, 0.74, 0.45);
const BUTTON: Color = Color::srgba(0.16, 0.17, 0.13, 0.92);
const BUTTON_HOVER: Color = Color::srgba(0.30, 0.31, 0.20, 0.95);
const BUTTON_PRESS: Color = Color::srgba(0.45, 0.44, 0.26, 1.0);
const FIELD: Color = Color::srgba(0.03, 0.04, 0.03, 0.95);
const SLIDER_TRACK: Color = Color::srgba(0.30, 0.30, 0.24, 0.9);
const SLIDER_THUMB_WIDTH: f32 = 12.0;

/// The bundled Rajdhani faces (SIL OFL 1.1, see `assets/fonts/OFL.txt`).
#[derive(Resource, Clone)]
pub struct UiFonts {
    pub regular: Handle<Font>,
    pub bold: Handle<Font>,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
        let regular = fonts.add(Font::from_bytes(
            include_bytes!("../../assets/fonts/Rajdhani-SemiBold.ttf").to_vec(),
        ));
        let bold = fonts.add(Font::from_bytes(
            include_bytes!("../../assets/fonts/Rajdhani-Bold.ttf").to_vec(),
        ));
        app.insert_resource(UiFonts { regular, bold })
            .add_systems(Update, (style_buttons, place_slider_thumbs));
    }
}

/// A menu button. Add your own marker component next to it to react to clicks.
#[derive(Component)]
pub struct MenuButton;

/// Large uppercase heading.
pub fn title(fonts: &UiFonts, text: &str, size: f32) -> impl Bundle {
    (
        Text::new(text.to_uppercase()),
        TextFont {
            font: fonts.bold.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(TEXT),
    )
}

/// Body text.
pub fn label(fonts: &UiFonts, text: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font: fonts.regular.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

/// A wide uppercase menu button; style comes from hover/press state.
pub fn button(fonts: &UiFonts, text: &str) -> impl Bundle {
    (
        MenuButton,
        Button,
        Hovered::default(),
        Node {
            min_width: px(280.0),
            padding: UiRect::axes(px(18.0), px(8.0)),
            border: UiRect::left(px(4.0)),
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(BUTTON),
        BorderColor::all(Color::NONE),
        children![(
            Text::new(text.to_uppercase()),
            TextFont {
                font: fonts.bold.clone().into(),
                font_size: FontSize::Px(26.0),
                ..default()
            },
            TextColor(TEXT),
        )],
    )
}

/// A bordered dark panel laid out as a column.
pub fn panel() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(24.0)),
            row_gap: px(10.0),
            border: UiRect::all(px(1.0)),
            ..default()
        },
        BackgroundColor(PANEL),
        BorderColor::all(PANEL_BORDER),
    )
}

/// A full-window root that darkens what's behind it and centres its children.
pub fn backdrop() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(12.0),
            ..default()
        },
        BackgroundColor(BACKDROP),
    )
}

/// Restyles every menu button from its hover/press state (few buttons, so every frame is fine).
fn style_buttons(
    mut buttons: Query<
        (
            &Hovered,
            Has<Pressed>,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<MenuButton>,
    >,
) {
    for (hovered, pressed, mut background, mut border) in &mut buttons {
        let (fill, edge) = if pressed {
            (BUTTON_PRESS, ACCENT)
        } else if hovered.0 {
            (BUTTON_HOVER, ACCENT)
        } else {
            (BUTTON, Color::NONE)
        };
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor::all(edge));
    }
}

/// A single-line text field (`bevy::ui_widgets::TextInput`) starting with `text`; click to
/// focus. Read the value from its `EditableText`.
pub fn text_field(fonts: &UiFonts, text: &str, max_characters: usize) -> impl Bundle {
    (
        TextInput,
        EditableText {
            max_characters: Some(max_characters),
            ..EditableText::new(text)
        },
        TextLayout::no_wrap(),
        TextFont {
            font: fonts.regular.clone().into(),
            font_size: FontSize::Px(24.0),
            ..default()
        },
        TextColor(TEXT),
        TextCursorStyle {
            color: ACCENT,
            ..default()
        },
        Node {
            width: px(280.0),
            padding: UiRect::axes(px(10.0), px(4.0)),
            border: UiRect::all(px(1.0)),
            ..default()
        },
        BackgroundColor(FIELD),
        BorderColor::all(PANEL_BORDER),
    )
}

/// A horizontal slider (`bevy::ui_widgets::Slider`) over `min..=max`. It updates its own
/// `SliderValue`; react to changes with an `On<ValueChange<f32>>` observer.
pub fn slider(value: f32, min: f32, max: f32) -> impl Bundle {
    (
        Slider::default(),
        SliderValue(value),
        SliderRange::new(min, max),
        Hovered::default(),
        observe(slider_self_update),
        Node {
            width: px(280.0),
            height: px(22.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![
            (
                Node {
                    height: px(4.0),
                    ..default()
                },
                BackgroundColor(SLIDER_TRACK),
            ),
            // Narrower than the slider by the thumb width, so the thumb's `left` is a plain
            // percentage of the value.
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0.0),
                    right: px(SLIDER_THUMB_WIDTH),
                    top: px(0.0),
                    bottom: px(0.0),
                    ..default()
                },
                children![(
                    SliderThumb,
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(SLIDER_THUMB_WIDTH),
                        height: percent(100.0),
                        ..default()
                    },
                    BackgroundColor(ACCENT),
                )],
            ),
        ],
    )
}

/// Moves each slider's thumb to its value.
fn place_slider_thumbs(
    sliders: Query<(Entity, &SliderValue, &SliderRange), Changed<SliderValue>>,
    children: Query<&Children>,
    mut thumbs: Query<&mut Node, With<SliderThumb>>,
) {
    for (slider, value, range) in &sliders {
        let left = percent(range.thumb_position(value.0) * 100.0);
        for child in children.iter_descendants(slider) {
            if let Ok(mut thumb) = thumbs.get_mut(child) {
                thumb.left = left;
            }
        }
    }
}
