//! Weapon HUD: centred crosshair (opening with spread, fading out when aiming), hitmarker,
//! the ammo counter with the held Weapon's name, the black scope mask of scoped Weapons, the
//! hold-breath vignette and the "click to rack" prompt of an open bolt.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::LetterSpacing;
use bevy::window::PrimaryWindow;

use super::{HeldWeapon, WeaponSystems, ammo, ease};
use crate::data::{ActiveRules, GameData};
use crate::equipment::Carried;
use crate::flow::{AppState, Settings};
use crate::net::protocol::{AmmoRefill, AmmoSource, HitConfirm};
use crate::player::{EyeCamera, LocalLife};
use crate::shared::{FromServer, LocalPlayer};
use crate::ui::{self, AMBER, UiFonts, hud_text};

/// Opacity of the crosshair at the hip.
const CROSSHAIR_ALPHA: f32 = 0.9;
const ARM_LENGTH: f32 = 8.0;
const ARM_THICKNESS: f32 = 2.0;
/// The dark outline around each crosshair arm, per side, so it reads on any Map.
const ARM_OUTLINE: f32 = 1.0;
const ARM_OUTLINE_ALPHA: f32 = 0.55;
/// Smallest gap between the centre and the arms, in logical pixels.
const CROSSHAIR_MIN_GAP: f32 = 4.0;
/// The crosshair fades out over this much of the aim-down-sights blend.
const CROSSHAIR_FADE_ADS: f32 = 0.5;

const HITMARKER_SECS: f32 = 0.22;
const HITMARKER_SIZE: f32 = 30.0;
const HITMARKER_ARM: Vec2 = Vec2::new(9.0, 2.5);
const HITMARKER_COLOR: Color = Color::WHITE;
const HITMARKER_KILL_COLOR: Color = Color::srgb(1.0, 0.15, 0.1);
/// How much bigger than at rest a hitmarker pops, for a hit and for a kill.
const HITMARKER_POP: f32 = 0.35;
const HITMARKER_KILL_POP: f32 = 0.7;

const AMMO_FONT_SIZE: f32 = 58.0;
const RESERVE_FONT_SIZE: f32 = 30.0;
const WEAPON_FONT_SIZE: f32 = 17.0;
const AMMO_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.95);
const AMMO_RESERVE_COLOR: Color = Color::srgba(0.82, 0.80, 0.72, 0.85);
const AMMO_LOW_COLOR: Color = Color::srgb(1.0, 0.35, 0.25);
/// Rounds in the magazine at or below which the count turns `AMMO_LOW_COLOR` (so does a Reserve of
/// one magazine or less: `ammo::reserve_low`).
const AMMO_LOW: u32 = 5;
const MODE_FONT_SIZE: f32 = 18.0;
const RELOAD_PROMPT_SIZE: f32 = 26.0;
const LOW_AMMO_PROMPT_SIZE: f32 = 20.0;
const RELOAD_PROMPT_COLOR: Color = Color::srgb(1.0, 0.3, 0.2);
/// The RELOAD and NO AMMO hints flash this many times a second.
const RELOAD_FLASH_HZ: f32 = 2.5;
const REFILL_CUE_SIZE: f32 = 20.0;
const REFILL_CUE_COLOR: Color = Color::srgb(0.55, 0.9, 0.4);
/// How long a pack's or a crate's "AMMO REFILLED" shows.
const REFILL_CUE_SECS: f32 = 1.5;
/// The Flag crate's RESUPPLYING stays up this many pulse intervals after the last pulse.
const RESUPPLY_HOLD_INTERVALS: f32 = 1.5;

pub(super) struct WeaponHudPlugin;

/// Side of the generated scope mask texture, in pixels.
const SCOPE_MASK_PX: u32 = 512;
/// The lens edge darkens from this fraction of the radius outward.
const SCOPE_VIGNETTE: f32 = 0.93;
const SCOPE_LINE: f32 = 2.0;
const SCOPE_POST: f32 = 7.0;
/// How far the thick posts reach in from the lens edge, in percent of its diameter.
const SCOPE_POST_LENGTH: f32 = 38.0;

/// Side of the generated breath vignette texture, in pixels.
const VIGNETTE_PX: u32 = 128;
/// The vignette starts this far out (1 is the screen's edge midpoints) and is full in the corners.
const VIGNETTE_INNER: f32 = 0.5;
/// Vignette opacity while holding breath (from a full tank to an empty one), and while out of
/// breath; it eases at this rate (1/s).
const HOLD_VIGNETTE_ALPHA: (f32, f32) = (0.25, 0.6);
const WINDED_VIGNETTE_ALPHA: f32 = 0.4;
const VIGNETTE_FOLLOW_PER_SEC: f32 = 8.0;
/// Out of breath, the vignette throbs this many times a second and this far (in alpha).
const WINDED_PULSE_HZ: f32 = 3.0;
const WINDED_PULSE_DEPTH: f32 = 0.1;
const WINDED_COLOR: (f32, f32, f32) = (0.35, 0.0, 0.0);
const RACK_PROMPT_SIZE: f32 = 18.0;

impl Plugin for WeaponHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::InGame), spawn_weapon_hud)
            .add_systems(
                Update,
                (
                    update_crosshair,
                    show_hitmarker,
                    fade_hitmarker,
                    update_ammo,
                    update_fire_mode,
                    update_ammo_prompt,
                    update_refill_cue,
                    update_scope,
                    update_breath_cue,
                    update_rack_prompt,
                )
                    .after(WeaponSystems)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[derive(Component)]
struct ScopeOverlay;

/// Dark edges while holding breath, reddish and throbbing while out of it.
#[derive(Component)]
struct BreathVignette;

#[derive(Component)]
struct RackPrompt;

/// The crosshair, centred: its arms sit at the edge of the hip spread cone on screen.
#[derive(Component)]
struct Crosshair;

/// One crosshair arm, pushed out from the centre along its unit direction. The `bool` marks
/// the dark outline arm drawn under the white one.
#[derive(Component)]
struct CrosshairArm(Vec2, bool);

#[derive(Component, Default)]
struct Hitmarker {
    left: f32,
    /// How far the pop-in of this hit starts (`HITMARKER_POP` or `HITMARKER_KILL_POP`).
    pop: f32,
}

/// One hitmarker arm; `true` marks the dark outline arm under the coloured one.
#[derive(Component)]
struct HitmarkerArm(bool);

#[derive(Component)]
struct AmmoCount;

/// The column above the ammo plate where `equipment::client` lists the carried equipment.
#[derive(Component)]
pub struct EquipmentRows;

#[derive(Component)]
struct WeaponLabel;

/// The Reserve beside the magazine count, e.g. `/ 120`.
#[derive(Component)]
struct ReserveCount;

/// The fire mode shown beside the ammo count.
#[derive(Component)]
struct FireModeLabel;

/// RELOAD or NO AMMO flashing, or a steady LOW AMMO, under the crosshair (`ammo::hint`).
#[derive(Component)]
struct AmmoPrompt;

/// RESUPPLYING at a held Flag, AMMO REFILLED after a pack or crate.
#[derive(Component)]
struct RefillCue;

fn crosshair_arm(direction: Vec2, outline: bool) -> impl Bundle {
    let mut size = if direction.x == 0.0 {
        Vec2::new(ARM_THICKNESS, ARM_LENGTH)
    } else {
        Vec2::new(ARM_LENGTH, ARM_THICKNESS)
    };
    let color = if outline {
        size += Vec2::splat(2.0 * ARM_OUTLINE);
        Color::BLACK.with_alpha(ARM_OUTLINE_ALPHA)
    } else {
        Color::WHITE.with_alpha(CROSSHAIR_ALPHA)
    };
    (
        CrosshairArm(direction, outline),
        Node {
            position_type: PositionType::Absolute,
            left: px(-size.x / 2.0),
            top: px(-size.y / 2.0),
            width: px(size.x),
            height: px(size.y),
            ..default()
        },
        BackgroundColor(color),
    )
}

fn hitmarker_arm(degrees: f32, outline: bool) -> impl Bundle {
    let size = if outline {
        HITMARKER_ARM + Vec2::splat(2.0)
    } else {
        HITMARKER_ARM
    };
    (
        HitmarkerArm(outline),
        Node {
            position_type: PositionType::Absolute,
            left: px((HITMARKER_SIZE - size.x) / 2.0),
            top: px((HITMARKER_SIZE - size.y) / 2.0),
            width: px(size.x),
            height: px(size.y),
            ..default()
        },
        // Pushed out from the centre along its own axis, leaving a gap in the middle.
        UiTransform {
            translation: Val2::px(
                degrees.to_radians().cos() * HITMARKER_SIZE / 3.0,
                degrees.to_radians().sin() * HITMARKER_SIZE / 3.0,
            ),
            rotation: Rot2::degrees(degrees),
            ..UiTransform::IDENTITY
        },
        BackgroundColor(if outline {
            Color::BLACK.with_alpha(0.6)
        } else {
            HITMARKER_COLOR
        }),
    )
}

fn centred() -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: percent(100.0),
        height: percent(100.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

/// A black texture with a transparent circle (the lens) and a soft dark rim.
fn scope_mask() -> Image {
    let size = SCOPE_MASK_PX;
    let radius = size as f32 / 2.0;
    let data = (0..size * size)
        .flat_map(|i| {
            let at = Vec2::new((i % size) as f32, (i / size) as f32) + 0.5 - radius;
            let edge = (at.length() / radius - SCOPE_VIGNETTE) / (1.0 - SCOPE_VIGNETTE);
            let alpha = edge.clamp(0.0, 1.0);
            [0, 0, 0, (alpha * alpha * 255.0) as u8]
        })
        .collect();
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// White with a transparent middle and an opaque rim: tinted and faded through the `ImageNode`.
fn breath_vignette() -> Image {
    let size = VIGNETTE_PX;
    let half = size as f32 / 2.0;
    let data = (0..size * size)
        .flat_map(|i| {
            let at = Vec2::new((i % size) as f32, (i / size) as f32) + 0.5 - half;
            let edge =
                ((at.length() / half - VIGNETTE_INNER) / (1.45 - VIGNETTE_INNER)).clamp(0.0, 1.0);
            [255, 255, 255, (ease(edge) * 255.0) as u8]
        })
        .collect();
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// The breath vignette and the rack prompt, above the scope view.
fn spawn_aim_cues(commands: &mut Commands, fonts: &UiFonts, vignette: Handle<Image>) {
    commands.spawn((
        Name::new("BreathVignette"),
        BreathVignette,
        DespawnOnExit(AppState::InGame),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            ..default()
        },
        ImageNode::new(vignette)
            .with_mode(NodeImageMode::Stretch)
            .with_color(Color::srgba(0.0, 0.0, 0.0, 0.0)),
    ));
    commands.spawn((
        Name::new("RackPrompt"),
        DespawnOnExit(AppState::InGame),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            top: percent(60.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            RackPrompt,
            hud_text(
                &fonts.bold,
                "CLICK TO RACK",
                RACK_PROMPT_SIZE,
                Color::srgba(1.0, 1.0, 1.0, 0.9)
            ),
            Visibility::Hidden,
        )],
    ));
    commands.spawn((
        Name::new("AmmoPrompt"),
        DespawnOnExit(AppState::InGame),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            top: percent(58.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            AmmoPrompt,
            hud_text(
                &fonts.display,
                "RELOAD",
                RELOAD_PROMPT_SIZE,
                RELOAD_PROMPT_COLOR
            ),
            Visibility::Hidden,
        )],
    ));
    commands.spawn((
        Name::new("RefillCue"),
        DespawnOnExit(AppState::InGame),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            top: percent(64.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            RefillCue,
            hud_text(
                &fonts.bold,
                "RESUPPLYING",
                REFILL_CUE_SIZE,
                REFILL_CUE_COLOR
            ),
            LetterSpacing::Px(2.0),
            Visibility::Hidden,
        )],
    ));
}

/// A black reticle bar placed inside the lens square.
fn reticle(node: Node) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            ..node
        },
        BackgroundColor(Color::BLACK),
    )
}

/// Full-window scope view: black bars either side of a square lens mask with a crosshair of
/// thin lines and thick posts, as on a CoD4 M40.
fn spawn_scope(commands: &mut Commands, mask: Handle<Image>) {
    let black = || {
        (
            Node {
                flex_grow: 1.0,
                height: percent(100.0),
                ..default()
            },
            BackgroundColor(Color::BLACK),
        )
    };
    let thin = SCOPE_LINE;
    let post = SCOPE_POST;
    commands.spawn((
        Name::new("Scope"),
        ScopeOverlay,
        DespawnOnExit(AppState::InGame),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            flex_direction: FlexDirection::Row,
            ..default()
        },
        Visibility::Hidden,
        children![
            black(),
            (
                Node {
                    height: percent(100.0),
                    aspect_ratio: Some(1.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                ImageNode::new(mask),
                children![
                    reticle(Node {
                        left: percent(0.0),
                        width: percent(100.0),
                        top: percent(50.0),
                        height: px(thin),
                        margin: UiRect::top(px(-thin / 2.0)),
                        ..default()
                    }),
                    reticle(Node {
                        top: percent(0.0),
                        height: percent(100.0),
                        left: percent(50.0),
                        width: px(thin),
                        margin: UiRect::left(px(-thin / 2.0)),
                        ..default()
                    }),
                    reticle(Node {
                        left: percent(0.0),
                        width: percent(SCOPE_POST_LENGTH),
                        top: percent(50.0),
                        height: px(post),
                        margin: UiRect::top(px(-post / 2.0)),
                        ..default()
                    }),
                    reticle(Node {
                        right: percent(0.0),
                        width: percent(SCOPE_POST_LENGTH),
                        top: percent(50.0),
                        height: px(post),
                        margin: UiRect::top(px(-post / 2.0)),
                        ..default()
                    }),
                    reticle(Node {
                        bottom: percent(0.0),
                        height: percent(SCOPE_POST_LENGTH),
                        left: percent(50.0),
                        width: px(post),
                        margin: UiRect::left(px(-post / 2.0)),
                        ..default()
                    }),
                ],
            ),
            black(),
        ],
    ));
}

fn spawn_weapon_hud(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    mut images: ResMut<Assets<Image>>,
) {
    spawn_scope(&mut commands, images.add(scope_mask()));
    spawn_aim_cues(&mut commands, &fonts, images.add(breath_vignette()));
    commands.spawn((
        Name::new("Crosshair"),
        DespawnOnExit(AppState::InGame),
        centred(),
        children![(
            Crosshair,
            Node::default(),
            children![
                crosshair_arm(Vec2::NEG_Y, true),
                crosshair_arm(Vec2::Y, true),
                crosshair_arm(Vec2::NEG_X, true),
                crosshair_arm(Vec2::X, true),
                crosshair_arm(Vec2::NEG_Y, false),
                crosshair_arm(Vec2::Y, false),
                crosshair_arm(Vec2::NEG_X, false),
                crosshair_arm(Vec2::X, false),
            ],
        )],
    ));
    commands.spawn((
        Name::new("Hitmarker"),
        DespawnOnExit(AppState::InGame),
        centred(),
        children![(
            Hitmarker::default(),
            Node {
                width: px(HITMARKER_SIZE),
                height: px(HITMARKER_SIZE),
                ..default()
            },
            UiTransform::IDENTITY,
            Visibility::Hidden,
            children![
                hitmarker_arm(45.0, true),
                hitmarker_arm(135.0, true),
                hitmarker_arm(225.0, true),
                hitmarker_arm(315.0, true),
                hitmarker_arm(45.0, false),
                hitmarker_arm(135.0, false),
                hitmarker_arm(225.0, false),
                hitmarker_arm(315.0, false),
            ],
        )],
    ));
    // The bottom-right cluster: the equipment rows (filled by `equipment::client`), then a plate
    // with the weapon, its fire mode and the rounds.
    commands.spawn((
        Name::new("Ammo"),
        DespawnOnExit(AppState::InGame),
        Node {
            position_type: PositionType::Absolute,
            right: px(0.0),
            bottom: px(0.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            padding: UiRect {
                left: px(48.0),
                right: px(28.0),
                top: px(10.0),
                bottom: px(16.0),
            },
            row_gap: px(2.0),
            ..default()
        },
        BackgroundGradient::from(LinearGradient::new(
            LinearGradient::TO_RIGHT,
            vec![
                ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, 0.0), 0.0),
                ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, 0.5), 100.0),
            ],
        )),
        children![
            (
                EquipmentRows,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexEnd,
                    row_gap: px(3.0),
                    margin: UiRect::bottom(px(8.0)),
                    ..default()
                },
            ),
            (
                Node {
                    width: px(150.0),
                    height: px(3.0),
                    ..default()
                },
                BackgroundColor(AMBER),
            ),
            (
                WeaponLabel,
                hud_text(&fonts.bold, "", WEAPON_FONT_SIZE, ui::TEXT_DIM),
                LetterSpacing::Px(1.5),
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::FlexEnd,
                    column_gap: px(6.0),
                    ..default()
                },
                children![
                    (
                        FireModeLabel,
                        hud_text(&fonts.bold, "", MODE_FONT_SIZE, AMBER),
                        LetterSpacing::Px(1.5),
                        Node {
                            margin: UiRect {
                                right: px(12.0),
                                bottom: px(9.0),
                                ..default()
                            },
                            ..default()
                        },
                    ),
                    (
                        AmmoCount,
                        hud_text(&fonts.display, "", AMMO_FONT_SIZE, AMMO_COLOR),
                    ),
                    (
                        ReserveCount,
                        hud_text(&fonts.bold, "", RESERVE_FONT_SIZE, AMMO_RESERVE_COLOR),
                        Node {
                            margin: UiRect::bottom(px(6.0)),
                            ..default()
                        },
                    ),
                ],
            ),
        ],
    ));
}

/// Opens the arms to the hip spread cone's edge on screen and fades them out with the sights.
/// Hidden throughout when the Match Rules have no crosshair.
fn update_crosshair(
    rules: ActiveRules,
    life: Res<LocalLife>,
    weapons: Query<&HeldWeapon, With<LocalPlayer>>,
    eye: Query<&Projection, With<EyeCamera>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut crosshair: Single<&mut Visibility, With<Crosshair>>,
    mut arms: Query<(&CrosshairArm, &mut UiTransform, &mut BackgroundColor)>,
) {
    let (Ok(weapon), Ok(Projection::Perspective(projection)), Ok(window)) =
        (weapons.single(), eye.single(), window.single())
    else {
        crosshair.set_if_neq(Visibility::Hidden);
        return;
    };
    let alpha = CROSSHAIR_ALPHA * (1.0 - weapon.ads / CROSSHAIR_FADE_ADS).clamp(0.0, 1.0);
    crosshair.set_if_neq(
        if alpha <= 0.0 || life.is_dead() || !rules.get().hud.crosshair {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        },
    );
    let spread = weapon.hip_spread_deg(&weapon.stats).to_radians();
    let gap = (spread.tan() / (projection.fov / 2.0).tan() * window.height() / 2.0)
        .max(CROSSHAIR_MIN_GAP);
    let offset = gap + ARM_LENGTH / 2.0;
    for (arm, mut transform, mut color) in &mut arms {
        let translation = Val2::px(arm.0.x * offset, arm.0.y * offset);
        if transform.translation != translation {
            transform.translation = translation;
        }
        let tint = if arm.1 {
            Color::BLACK.with_alpha(ARM_OUTLINE_ALPHA * alpha / CROSSHAIR_ALPHA)
        } else {
            Color::WHITE.with_alpha(alpha)
        };
        color.set_if_neq(BackgroundColor(tint));
    }
}

/// White on a hit, red on a kill, each only if the Match Rules show it.
fn show_hitmarker(
    rules: ActiveRules,
    mut hits: MessageReader<FromServer<HitConfirm>>,
    mut marker: Single<(&mut Hitmarker, &mut Visibility)>,
    mut arms: Query<(&HitmarkerArm, &mut BackgroundColor)>,
) {
    let Some(kill) = hits
        .read()
        .map(|FromServer(hit)| hit.kill)
        .reduce(|a, b| a || b)
    else {
        return;
    };
    let hud = &rules.get().hud;
    let killed = kill && hud.kill_hitmarker;
    let color = if killed {
        HITMARKER_KILL_COLOR
    } else if hud.hitmarkers {
        HITMARKER_COLOR
    } else {
        return;
    };
    let (hitmarker, visibility) = &mut *marker;
    hitmarker.left = HITMARKER_SECS;
    hitmarker.pop = if killed {
        HITMARKER_KILL_POP
    } else {
        HITMARKER_POP
    };
    **visibility = Visibility::Inherited;
    for (arm, mut background) in &mut arms {
        if !arm.0 {
            background.0 = color;
        }
    }
}

/// Pops the hitmarker in and shrinks it back while it fades (quicker to read than a blink).
fn fade_hitmarker(
    time: Res<Time>,
    mut marker: Single<(&mut Hitmarker, &mut Visibility, &mut UiTransform)>,
    mut arms: Query<(&HitmarkerArm, &mut BackgroundColor)>,
) {
    let (hitmarker, visibility, transform) = &mut *marker;
    if hitmarker.left <= 0.0 {
        return;
    }
    hitmarker.left -= time.delta_secs();
    if hitmarker.left <= 0.0 {
        **visibility = Visibility::Hidden;
        transform.scale = Vec2::ONE;
        return;
    }
    let t = hitmarker.left / HITMARKER_SECS;
    transform.scale = Vec2::splat(1.0 + hitmarker.pop * t * t);
    let fade = (t * 2.0).min(1.0);
    for (arm, mut background) in &mut arms {
        let alpha = if arm.0 { 0.6 * fade } else { fade };
        background.0 = background.0.with_alpha(alpha);
    }
}

/// Rewrites the counter only when the Weapon, rounds, Reserve or reload state shown actually
/// change: `30 / 120`, the Reserve in the low-ammo colour at one magazine or less. With the
/// launcher selected it counts the 40mm rounds left instead, which have no Reserve.
fn update_ammo(
    weapons: Query<(&HeldWeapon, Option<&Carried>), With<LocalPlayer>>,
    mut shown: Local<Option<AmmoShown>>,
    mut count: Single<
        (&mut Text, &mut TextColor),
        (With<AmmoCount>, Without<WeaponLabel>, Without<ReserveCount>),
    >,
    mut reserve_text: Single<
        (&mut Text, &mut TextColor),
        (With<ReserveCount>, Without<WeaponLabel>, Without<AmmoCount>),
    >,
    mut label: Single<&mut Text, (With<WeaponLabel>, Without<AmmoCount>, Without<ReserveCount>)>,
) {
    let Ok((weapon, carried)) = weapons.single() else {
        return;
    };
    let launcher = weapon.launcher_mode();
    let rounds = if launcher {
        carried
            .and_then(|carried| carried.launcher)
            .map_or(0, |pouch| pouch.left)
    } else {
        weapon.rounds
    };
    let reloading = !launcher && weapon.reload_left.is_some();
    let state = AmmoShown {
        weapon: weapon.id,
        rounds,
        reserve: weapon.reserve,
        reloading,
        launcher,
    };
    if *shown == Some(state) && !label.is_added() {
        return;
    }
    *shown = Some(state);
    let (text, color) = &mut *count;
    text.0 = rounds.to_string();
    let low = if launcher {
        rounds == 0
    } else {
        rounds <= AMMO_LOW
    };
    color.0 = if low { AMMO_LOW_COLOR } else { AMMO_COLOR };
    let (text, color) = &mut *reserve_text;
    if launcher {
        text.0.clear();
    } else {
        text.0 = format!("/ {}", weapon.reserve);
        color.0 = if ammo::reserve_low(weapon.reserve, &weapon.stats) {
            AMMO_LOW_COLOR
        } else {
            AMMO_RESERVE_COLOR
        };
    }
    let name = weapon.stats.name.to_uppercase();
    label.0 = if reloading {
        format!("{name}  \u{b7}  RELOADING")
    } else {
        name
    };
}

/// What `update_ammo` last wrote.
#[derive(Clone, Copy, PartialEq)]
struct AmmoShown {
    weapon: crate::data::WeaponId,
    rounds: u32,
    reserve: u32,
    reloading: bool,
    launcher: bool,
}

/// The label shown for the launcher mode where the fire mode goes.
const LAUNCHER_LABEL: &str = "40MM";

/// The current fire mode, e.g. `AUTO` or `BURST 3`, beside the ammo count; `40MM` while the
/// underslung launcher is selected.
fn update_fire_mode(
    weapons: Query<&HeldWeapon, With<LocalPlayer>>,
    mut label: Single<&mut Text, With<FireModeLabel>>,
) {
    let Ok(weapon) = weapons.single() else {
        return;
    };
    let shown = if weapon.launcher_mode() {
        LAUNCHER_LABEL.to_string()
    } else {
        weapon.mode.label()
    };
    if label.0 != shown {
        label.0 = shown;
    }
}

/// The hint under the crosshair (`ammo::hint`): a flashing RELOAD while the magazine is empty and
/// nothing reloads it (with Auto reload on a pull does that), a flashing NO AMMO with the magazine
/// and the Reserve both empty, a steady LOW AMMO with one magazine or less in the Reserve. The
/// launcher has no Reserve and no hint.
fn update_ammo_prompt(
    time: Res<Time>,
    settings: Res<Settings>,
    life: Res<LocalLife>,
    weapons: Query<&HeldWeapon, With<LocalPlayer>>,
    mut prompt: Single<
        (&mut Visibility, &mut Text, &mut TextColor, &mut TextFont),
        With<AmmoPrompt>,
    >,
) {
    let hint = weapons
        .single()
        .ok()
        .filter(|weapon| !life.is_dead() && !weapon.launcher_mode())
        .and_then(|weapon| {
            ammo::hint(
                weapon.rounds,
                weapon.reserve,
                weapon.reload_left.is_some(),
                settings.auto_reload,
                &weapon.stats,
            )
        });
    let flash = (time.elapsed_secs() * RELOAD_FLASH_HZ).fract() < 0.6;
    let (visibility, text, color, font) = &mut *prompt;
    let (shown, words, tint, size) = match hint {
        Some(ammo::Hint::Reload) => (flash, "RELOAD", RELOAD_PROMPT_COLOR, RELOAD_PROMPT_SIZE),
        Some(ammo::Hint::NoAmmo) => (flash, "NO AMMO", RELOAD_PROMPT_COLOR, RELOAD_PROMPT_SIZE),
        Some(ammo::Hint::LowAmmo) => (true, "LOW AMMO", AMBER, LOW_AMMO_PROMPT_SIZE),
        None => (false, "", RELOAD_PROMPT_COLOR, RELOAD_PROMPT_SIZE),
    };
    visibility.set_if_neq(if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if !words.is_empty() && text.0 != words {
        text.0 = words.to_owned();
    }
    color.set_if_neq(TextColor(tint));
    if font.font_size != FontSize::Px(size) {
        font.font_size = FontSize::Px(size);
    }
}

/// What the last refill said: for how long and which words.
struct Cue {
    left: f32,
    words: &'static str,
}

/// RESUPPLYING while the crate at a Flag keeps handing out ammo (it stays up a little longer than
/// one pulse interval), AMMO REFILLED for a moment after a pack or a Team Deathmatch crate.
fn update_refill_cue(
    time: Res<Time>,
    data: Res<GameData>,
    life: Res<LocalLife>,
    mut refills: MessageReader<FromServer<AmmoRefill>>,
    mut cue: Local<Option<Cue>>,
    mut node: Single<(&mut Visibility, &mut Text), With<RefillCue>>,
) {
    for FromServer(refill) in refills.read() {
        *cue = Some(match refill.source {
            AmmoSource::Flag => Cue {
                left: data.domination().resupply.interval_secs * RESUPPLY_HOLD_INTERVALS,
                words: "RESUPPLYING",
            },
            AmmoSource::Pack | AmmoSource::Crate => Cue {
                left: REFILL_CUE_SECS,
                words: "AMMO REFILLED",
            },
        });
    }
    if life.is_dead() {
        *cue = None;
    }
    if let Some(active) = cue.as_mut() {
        active.left -= time.delta_secs();
    }
    if cue.as_ref().is_some_and(|active| active.left <= 0.0) {
        *cue = None;
    }
    let (visibility, text) = &mut *node;
    match cue.as_ref() {
        Some(active) => {
            if text.0 != active.words {
                text.0 = active.words.to_owned();
            }
            visibility.set_if_neq(Visibility::Inherited);
        }
        None => {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

/// Shows the scope mask while a scoped Weapon is aimed far enough in.
fn update_scope(
    life: Res<LocalLife>,
    weapons: Query<&HeldWeapon, With<LocalPlayer>>,
    mut scope: Single<&mut Visibility, With<ScopeOverlay>>,
) {
    let scoped = weapons
        .single()
        .is_ok_and(|weapon| !life.is_dead() && weapon.scoped(&weapon.stats));
    scope.set_if_neq(if scoped {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

/// Eases the vignette toward dark edges while holding breath or toward a throbbing red while
/// out of breath, and away again otherwise.
fn update_breath_cue(
    time: Res<Time>,
    life: Res<LocalLife>,
    weapons: Query<&HeldWeapon, With<LocalPlayer>>,
    mut vignette: Single<&mut ImageNode, With<BreathVignette>>,
) {
    let breath = weapons
        .single()
        .ok()
        .filter(|_| !life.is_dead())
        .map(|weapon| weapon.breath);
    let (rgb, alpha) = match breath {
        Some(breath) if breath.winded() => {
            let pulse = (time.elapsed_secs() * WINDED_PULSE_HZ * std::f32::consts::TAU).sin();
            (
                WINDED_COLOR,
                WINDED_VIGNETTE_ALPHA + pulse * WINDED_PULSE_DEPTH,
            )
        }
        Some(breath) if breath.holding() => {
            let (full, empty) = HOLD_VIGNETTE_ALPHA;
            ((0.0, 0.0, 0.0), full.lerp(empty, 1.0 - breath.tank()))
        }
        _ => ((0.0, 0.0, 0.0), 0.0),
    };
    let mut color = vignette.color.to_srgba();
    color
        .alpha
        .smooth_nudge(&alpha, VIGNETTE_FOLLOW_PER_SEC, time.delta_secs());
    (color.red, color.green, color.blue) = rgb;
    vignette.color = color.into();
}

/// "CLICK TO RACK" while looking through the scope with the bolt open.
fn update_rack_prompt(
    life: Res<LocalLife>,
    weapons: Query<&HeldWeapon, With<LocalPlayer>>,
    mut prompt: Single<&mut Visibility, With<RackPrompt>>,
) {
    let open = weapons.single().is_ok_and(|weapon| {
        !life.is_dead() && weapon.scoped(&weapon.stats) && weapon.bolt.waiting_for_click()
    });
    prompt.set_if_neq(if open {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}
