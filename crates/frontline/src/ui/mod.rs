//! Shared look of every menu screen (CoD4-ish): fonts, palette and widgets. Client/Host only.
//!
//! Build screens from [`title`], [`label`], [`button`], [`compact_button`], [`panel`],
//! [`backdrop`], [`slider`] and [`text_field`]; [`spawn_result_banner`] shows a result
//! (ROUND WON, VICTORY) over the game; buttons get hover/press styling automatically. React to a click with an
//! observer on the button entity:
//! `commands.spawn(button(&fonts, "Host Game")).observe(|_: On<Activate>, ...| { ... })`
//! (`bevy::ui_widgets::Activate`; also fires on Enter/Space when the button has focus).

use bevy::asset::RenderAssetUsages;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{EditableText, LetterSpacing, TextCursorStyle};
use bevy::ui::{InteractionDisabled, Pressed};
use bevy::ui_widgets::{
    Button, Slider, SliderRange, SliderThumb, SliderValue, TextInput, observe, slider_self_update,
};

mod banner;

pub use banner::{BannerSpec, spawn_result_banner};

pub const BACKDROP: Color = Color::srgba(0.015, 0.02, 0.015, 0.84);
pub const PANEL: Color = Color::srgba(0.045, 0.05, 0.04, 0.9);
pub const PANEL_BORDER: Color = Color::srgba(0.50, 0.48, 0.34, 0.45);
pub const TEXT: Color = Color::srgb(0.92, 0.90, 0.82);
pub const TEXT_DIM: Color = Color::srgb(0.62, 0.60, 0.53);
/// Olive/khaki accent for highlights and selection.
pub const ACCENT: Color = Color::srgb(0.78, 0.74, 0.45);
/// Hot amber for what the player must notice (fire mode, hovered button, the thin rules).
pub const AMBER: Color = Color::srgb(0.96, 0.68, 0.18);
/// Near-opaque backdrop of the full-screen game menus (Loadout, Create a Class), which hide the
/// HUD behind them.
pub const BACKDROP_SOLID: Color = Color::srgba(0.015, 0.02, 0.015, 0.95);
const BUTTON: Color = Color::srgba(0.03, 0.035, 0.028, 0.80);
const BUTTON_EDGE: Color = Color::srgba(0.36, 0.35, 0.24, 0.9);
const BUTTON_HOVER: Color = Color::srgba(0.24, 0.21, 0.09, 0.92);
const BUTTON_PRESS: Color = Color::srgba(0.46, 0.36, 0.10, 1.0);
const FIELD: Color = Color::srgba(0.02, 0.025, 0.02, 0.95);
const SLIDER_TRACK: Color = Color::srgba(0.30, 0.30, 0.24, 0.9);
const SLIDER_THUMB_WIDTH: f32 = 12.0;

/// The bundled faces (SIL OFL 1.1, see `assets/fonts/CREDITS.md`): Rajdhani for text and Saira
/// Stencil One for titles and big numbers.
#[derive(Resource, Clone)]
pub struct UiFonts {
    pub regular: Handle<Font>,
    pub bold: Handle<Font>,
    /// The stencil face: headings, the logo, big HUD numbers.
    pub display: Handle<Font>,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
        let regular = fonts.add(Font::from_bytes(
            include_bytes!("../../assets/fonts/Rajdhani-SemiBold.ttf").to_vec(),
        ));
        let display = fonts.add(Font::from_bytes(
            include_bytes!("../../assets/fonts/SairaStencilOne-Regular.ttf").to_vec(),
        ));
        let bold = fonts.add(Font::from_bytes(
            include_bytes!("../../assets/fonts/Rajdhani-Bold.ttf").to_vec(),
        ));
        let icon = ArrowIcon(
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(arrow_image()),
        );
        app.insert_resource(UiFonts {
            regular,
            bold,
            display,
        })
        .insert_resource(icon)
        .add_systems(
            Update,
            (
                style_buttons,
                dim_disabled_buttons,
                place_slider_thumbs,
                banner::animate_banners,
            ),
        );
    }
}

/// A menu button. Add your own marker component next to it to react to clicks.
#[derive(Component)]
pub struct MenuButton;

/// Large uppercase heading in the stencil face.
pub fn title(fonts: &UiFonts, text: &str, size: f32) -> impl Bundle {
    (
        Text::new(text.to_uppercase()),
        TextFont {
            font: fonts.display.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        LetterSpacing::Px(size * 0.03),
        TextColor(TEXT),
        TextShadow {
            offset: Vec2::splat((size * 0.05).max(1.0)),
            color: Color::srgba(0.0, 0.0, 0.0, 0.7),
        },
    )
}

/// A thin full-width rule line (1 px): the sharp divider of panels, headers and lists.
pub fn rule(color: Color) -> impl Bundle {
    (
        Node {
            width: percent(100.0),
            height: px(1.0),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(color),
    )
}

/// HUD text: the readable face with the dark drop shadow CoD's HUD uses, so it holds up on any
/// map.
pub fn hud_text(font: &Handle<Font>, text: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        TextShadow {
            offset: Vec2::splat(1.5),
            color: Color::srgba(0.0, 0.0, 0.0, 0.85),
        },
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
        BorderColor::all(BUTTON_EDGE),
        children![(
            Text::new(text.to_uppercase()),
            TextFont {
                font: fonts.bold.clone().into(),
                font_size: FontSize::Px(26.0),
                ..default()
            },
            LetterSpacing::Px(1.2),
            TextColor(TEXT),
        )],
    )
}

/// A small menu button for dense screens (list rows, cycling pickers). Styled like [`button`].
pub fn compact_button(fonts: &UiFonts, text: &str, min_width: f32, size: f32) -> impl Bundle {
    (
        MenuButton,
        Button,
        Hovered::default(),
        Node {
            min_width: px(min_width),
            padding: UiRect::axes(px(10.0), px(3.0)),
            border: UiRect::left(px(3.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(BUTTON),
        BorderColor::all(BUTTON_EDGE),
        children![(
            Text::new(text.to_uppercase()),
            TextFont {
                font: fonts.bold.clone().into(),
                font_size: FontSize::Px(size),
                ..default()
            },
            LetterSpacing::Px(0.8),
            TextColor(TEXT),
        )],
    )
}

/// A white triangle pointing left, for the ◀ ▶ arrows of pickers: the menu font has no arrow
/// glyphs, so they are drawn (and flipped for ▶) as images.
#[derive(Resource, Clone)]
pub struct ArrowIcon(Handle<Image>);

/// A square arrow button, ◀ (or ▶ when `right`); styled like [`compact_button`].
pub fn arrow_button(icon: &ArrowIcon, right: bool) -> impl Bundle {
    (
        MenuButton,
        Button,
        Hovered::default(),
        Node {
            width: px(30.0),
            height: px(28.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(BUTTON),
        BorderColor::all(Color::NONE),
        children![(
            ImageNode {
                color: TEXT,
                flip_x: right,
                ..ImageNode::new(icon.0.clone())
            },
            Node {
                width: px(12.0),
                height: px(14.0),
                ..default()
            },
        )],
    )
}

/// The triangle of [`ArrowIcon`]: apex on the left, 32 x 32 px, edges anti-aliased by 4 x 4
/// supersampling.
fn arrow_image() -> Image {
    const SIZE: u32 = 32;
    const SAMPLES: u32 = 4;
    let inside = |x: f32, y: f32| {
        // Apex (3, 16), base from (29, 3) to (29, 29).
        let along = (x - 3.0) / 26.0;
        (0.0..=1.0).contains(&along) && (y - 16.0).abs() <= 13.0 * along
    };
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let hits = (0..SAMPLES * SAMPLES)
                .filter(|s| {
                    let sx = x as f32 + ((s % SAMPLES) as f32 + 0.5) / SAMPLES as f32;
                    let sy = y as f32 + ((s / SAMPLES) as f32 + 0.5) / SAMPLES as f32;
                    inside(sx, sy)
                })
                .count();
            let alpha = (hits as f32 / (SAMPLES * SAMPLES) as f32 * 255.0).round() as u8;
            pixels.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }
    Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// A bordered dark panel laid out as a column, with a heavy amber rule along its top edge.
pub fn panel() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(24.0)),
            row_gap: px(10.0),
            border: UiRect {
                top: px(3.0),
                ..UiRect::all(px(1.0))
            },
            ..default()
        },
        BackgroundColor(PANEL),
        BorderColor {
            top: AMBER,
            ..BorderColor::all(PANEL_BORDER)
        },
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
/// A button with `InteractionDisabled` (it doesn't activate) keeps its idle look.
fn style_buttons(
    mut buttons: Query<
        (
            &Hovered,
            Has<Pressed>,
            Has<InteractionDisabled>,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<MenuButton>,
    >,
) {
    for (hovered, pressed, disabled, mut background, mut border) in &mut buttons {
        let (fill, edge) = if disabled {
            (BUTTON, BUTTON_EDGE)
        } else if pressed {
            (BUTTON_PRESS, AMBER)
        } else if hovered.0 {
            (BUTTON_HOVER, AMBER)
        } else {
            (BUTTON, BUTTON_EDGE)
        };
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor::all(edge));
    }
}

/// Greys the label of a button while it is `InteractionDisabled` and gives it back its colour
/// when it is enabled again.
fn dim_disabled_buttons(
    disabled: Query<&Children, (With<MenuButton>, Added<InteractionDisabled>)>,
    mut enabled: RemovedComponents<InteractionDisabled>,
    buttons: Query<&Children, With<MenuButton>>,
    mut texts: Query<&mut TextColor>,
) {
    for (children, color) in disabled.iter().map(|children| (children, TEXT_DIM)).chain(
        enabled
            .read()
            .filter_map(|button| buttons.get(button).ok())
            .map(|children| (children, TEXT)),
    ) {
        for child in children {
            if let Ok(mut text) = texts.get_mut(*child) {
                text.0 = color;
            }
        }
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
