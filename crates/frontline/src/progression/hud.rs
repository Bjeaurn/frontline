//! Progression HUD: the CoD-style "+100 Kill" popups stacked under the crosshair, and the small
//! LEVEL UP and CAMO UNLOCKED toasts (the result banner, scaled down).

use bevy::prelude::*;

use super::client::{CamoUnlocked, LevelUp, XpGained};
use crate::data::GameData;
use crate::flow::AppState;
use crate::ui::{self, BannerSpec, UiFonts};

/// Popups on screen at once; a newer one pushes the oldest out.
const MAX_POPUPS: usize = 6;
/// A popup is fully shown this long, then fades out over `POPUP_FADE_SECS`.
const POPUP_HOLD_SECS: f32 = 1.5;
const POPUP_FADE_SECS: f32 = 0.6;
const POPUP_SIZE: f32 = 26.0;
/// Gap between the screen centre and the first popup.
const POPUP_TOP_GAP: f32 = 56.0;

/// Size of the toasts relative to a full result banner.
const TOAST_SCALE: f32 = 0.5;
const TOAST_HOLD_SECS: f32 = 3.0;
const TOAST_Z: i32 = 14;
const LEVEL_UP_TOP_PERCENT: f32 = 18.0;
const CAMO_TOP_PERCENT: f32 = 27.0;
const CAMO_COLOR: Color = Color::srgb(0.95, 0.78, 0.30);

pub(super) struct ProgressionHudPlugin;

impl Plugin for ProgressionHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::InGame), spawn_popup_stack)
            .add_systems(
                Update,
                (show_popups, animate_popups, show_toasts)
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

/// The column the popups are spawned into, centred under the crosshair.
#[derive(Component)]
struct PopupStack;

#[derive(Component)]
struct XpPopup {
    age: f32,
}

fn spawn_popup_stack(mut commands: Commands) {
    commands.spawn((
        Name::new("XpPopups"),
        PopupStack,
        DespawnOnExit(AppState::InGame),
        Node {
            position_type: PositionType::Absolute,
            top: percent(50.0),
            margin: UiRect::top(px(POPUP_TOP_GAP)),
            width: percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            ..default()
        },
        GlobalZIndex(8),
    ));
}

/// One "+100  KILL" line per award; an award worth nothing (a kill counted only for a camo
/// challenge) shows nothing.
fn show_popups(
    mut gained: MessageReader<XpGained>,
    fonts: Res<UiFonts>,
    stack: Single<(Entity, Option<&Children>), With<PopupStack>>,
    mut commands: Commands,
) {
    let (stack, children) = *stack;
    let mut shown = children.map_or(0, |children| children.len());
    for award in gained.read().filter(|award| award.amount > 0) {
        if shown >= MAX_POPUPS
            && let Some(oldest) = children.and_then(|children| children.first())
        {
            commands.entity(*oldest).despawn();
            shown -= 1;
        }
        shown += 1;
        commands.spawn((
            XpPopup { age: 0.0 },
            Text::new(format!(
                "+{}  {}",
                award.amount,
                award.reason.label().to_uppercase()
            )),
            TextFont {
                font: fonts.bold.clone().into(),
                font_size: FontSize::Px(POPUP_SIZE),
                ..default()
            },
            TextColor(ui::ACCENT),
            ChildOf(stack),
        ));
    }
}

fn animate_popups(
    time: Res<Time<Real>>,
    mut popups: Query<(Entity, &mut XpPopup, &mut TextColor)>,
    mut commands: Commands,
) {
    for (entity, mut popup, mut color) in &mut popups {
        popup.age += time.delta_secs();
        if popup.age >= POPUP_HOLD_SECS + POPUP_FADE_SECS {
            commands.entity(entity).despawn();
            continue;
        }
        let alpha = 1.0 - ((popup.age - POPUP_HOLD_SECS) / POPUP_FADE_SECS).clamp(0.0, 1.0);
        color.0 = ui::ACCENT.with_alpha(alpha);
    }
}

fn show_toasts(
    mut level_ups: MessageReader<LevelUp>,
    mut camos: MessageReader<CamoUnlocked>,
    data: Res<GameData>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
) {
    for level_up in level_ups.read() {
        let subline = if level_up.unlocked.is_empty() {
            format!("Level {}", level_up.level)
        } else {
            format!(
                "Level {}  -  unlocked {}",
                level_up.level,
                level_up.unlocked.join(", ")
            )
        };
        let banner = ui::spawn_result_banner(
            &mut commands,
            &fonts,
            &BannerSpec {
                headline: "Level up",
                color: ui::ACCENT,
                subline: &subline,
                top_percent: LEVEL_UP_TOP_PERCENT,
                scale: TOAST_SCALE,
                hold_secs: TOAST_HOLD_SECS,
                z_index: TOAST_Z,
            },
        );
        commands
            .entity(banner)
            .insert(DespawnOnExit(AppState::InGame));
    }
    for unlocked in camos.read() {
        let name = data
            .progression
            .camo(&unlocked.camo)
            .map_or(unlocked.camo.as_str(), |camo| camo.name.as_str());
        let subline = format!("{name}  -  {}", data.weapon(unlocked.weapon).name);
        let banner = ui::spawn_result_banner(
            &mut commands,
            &fonts,
            &BannerSpec {
                headline: "Camo unlocked",
                color: CAMO_COLOR,
                subline: &subline,
                top_percent: CAMO_TOP_PERCENT,
                scale: TOAST_SCALE,
                hold_secs: TOAST_HOLD_SECS,
                z_index: TOAST_Z,
            },
        );
        commands
            .entity(banner)
            .insert(DespawnOnExit(AppState::InGame));
    }
}
