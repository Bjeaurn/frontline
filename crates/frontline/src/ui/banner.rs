//! A result banner: a black bar that grows outward from a sliver in the middle of the screen,
//! then fades in a big headline with a smaller subline under it, holds, and fades out.
//! `spawn_result_banner` is all a screen needs; `animate_banners` (added by `UiPlugin`) runs it.

use bevy::prelude::*;
use bevy::text::LetterSpacing;

use super::UiFonts;

/// Height the bar grows to, in logical pixels.
const BAR_HEIGHT: f32 = 140.0;
const SLIVER_HEIGHT: f32 = 4.0;
const BAR_ALPHA: f32 = 0.66;
const GROW_SECS: f32 = 0.3;
/// The text starts fading in this far into the grow, and takes `TEXT_FADE_SECS`.
const TEXT_DELAY_SECS: f32 = 0.25;
const TEXT_FADE_SECS: f32 = 0.2;
const FADE_OUT_SECS: f32 = 0.5;
const HEADLINE_SIZE: f32 = 72.0;
const SUBLINE_SIZE: f32 = 26.0;
const SUBLINE_COLOR: Color = Color::srgb(0.92, 0.9, 0.84);

/// What to show.
pub struct BannerSpec<'a> {
    pub headline: &'a str,
    /// Colour of the headline.
    pub color: Color,
    pub subline: &'a str,
    /// Distance of the banner's top edge from the top of the screen, in percent.
    pub top_percent: f32,
    /// Size relative to a full banner (1.0): the bar, headline and subline scale with it.
    pub scale: f32,
    /// Seconds fully shown, after the grow and before the fade-out.
    pub hold_secs: f32,
    pub z_index: i32,
}

#[derive(Component)]
pub struct ResultBanner {
    age: f32,
    scale: f32,
    hold_secs: f32,
    color: Color,
    bar: Entity,
    texts: [Entity; 2],
}

/// Spawns a banner; it despawns itself once it has faded out. `DespawnOnExit` is left to the
/// caller (`.insert(..)` on the returned entity).
pub fn spawn_result_banner(commands: &mut Commands, fonts: &UiFonts, spec: &BannerSpec) -> Entity {
    let bar = commands
        .spawn((
            Node {
                width: px(0.0),
                height: px(SLIVER_HEIGHT),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, BAR_ALPHA)),
        ))
        .id();
    let text = |commands: &mut Commands, text: &str, size: f32, color: Color, bold: bool| {
        commands
            .spawn((
                Text::new(text),
                TextFont {
                    font: (if bold {
                        fonts.display.clone()
                    } else {
                        fonts.bold.clone()
                    })
                    .into(),
                    font_size: FontSize::Px(size),
                    ..default()
                },
                LetterSpacing::Px(if bold { size * 0.04 } else { 3.0 }),
                TextColor(color.with_alpha(0.0)),
            ))
            .id()
    };
    // The subline shrinks half as fast as the rest, so it stays readable on a small banner.
    let subline_size = SUBLINE_SIZE * (0.5 + 0.5 * spec.scale);
    let headline = text(
        commands,
        &spec.headline.to_uppercase(),
        HEADLINE_SIZE * spec.scale,
        spec.color,
        true,
    );
    let subline = text(commands, spec.subline, subline_size, SUBLINE_COLOR, false);
    let words = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .add_children(&[headline, subline])
        .id();
    commands
        .spawn((
            Name::new("Result banner"),
            ResultBanner {
                age: 0.0,
                scale: spec.scale,
                hold_secs: spec.hold_secs,
                color: spec.color,
                bar,
                texts: [headline, subline],
            },
            Node {
                position_type: PositionType::Absolute,
                top: percent(spec.top_percent),
                width: percent(100.0),
                height: px(BAR_HEIGHT * spec.scale),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            GlobalZIndex(spec.z_index),
        ))
        .add_children(&[bar, words])
        .id()
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

pub(super) fn animate_banners(
    time: Res<Time<Real>>,
    mut banners: Query<(Entity, &mut ResultBanner)>,
    mut nodes: Query<&mut Node>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut texts: Query<&mut TextColor>,
    mut commands: Commands,
) {
    for (entity, mut banner) in &mut banners {
        banner.age += time.delta_secs();
        let age = banner.age;
        let fade_start = GROW_SECS + banner.hold_secs;
        if age >= fade_start + FADE_OUT_SECS {
            commands.entity(entity).despawn();
            continue;
        }
        let out = 1.0 - ((age - fade_start) / FADE_OUT_SECS).clamp(0.0, 1.0);
        let grown = ease_out(age / GROW_SECS);
        if let Ok(mut node) = nodes.get_mut(banner.bar) {
            node.width = percent(grown * 100.0);
            let full = BAR_HEIGHT * banner.scale;
            node.height = px(SLIVER_HEIGHT + (full - SLIVER_HEIGHT) * grown);
        }
        if let Ok(mut background) = backgrounds.get_mut(banner.bar) {
            background.0 = Color::srgba(0.0, 0.0, 0.0, BAR_ALPHA * out);
        }
        let text_in = ((age - TEXT_DELAY_SECS) / TEXT_FADE_SECS).clamp(0.0, 1.0) * out;
        for (entity, base) in banner.texts.into_iter().zip([banner.color, SUBLINE_COLOR]) {
            if let Ok(mut color) = texts.get_mut(entity) {
                color.0 = base.with_alpha(text_in);
            }
        }
    }
}
