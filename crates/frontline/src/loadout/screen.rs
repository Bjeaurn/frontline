//! The Loadout screen (`Overlay::Loadout`): Default Loadouts on the left, the selected one's
//! Weapon and stat bars on the right. CHOOSE confirms, Esc goes back. B opens it in game.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{ChosenLoadout, SpawnClock};
use crate::data::{FireMode, GameData, WeaponStats};
use crate::flow::{Overlay, Settings};
use crate::player::LocalLife;
use crate::shared::LocalPlayer;
use crate::ui::{self, UiFonts};
use crate::weapon::HeldWeapon;

/// Opens the Loadout screen during play.
const OPEN_KEY: KeyCode = KeyCode::KeyB;
/// Shortest stat bar, so the weakest Weapon still shows a sliver.
const MIN_BAR: f32 = 0.12;
const BAR_WIDTH: f32 = 260.0;
const BAR_HEIGHT: f32 = 10.0;
const BAR_TRACK: Color = Color::srgba(1.0, 1.0, 1.0, 0.12);

pub(super) struct LoadoutScreenPlugin;

impl Plugin for LoadoutScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Preview>()
            .init_resource::<ReturnTo>()
            .add_systems(OnEnter(Overlay::Loadout), spawn_screen)
            .add_systems(
                Update,
                (
                    remember_origin,
                    open_with_key.run_if(in_state(Overlay::None)),
                    (back_on_esc, refresh).run_if(in_state(Overlay::Loadout)),
                )
                    .chain(),
            );
    }
}

/// The Loadout highlighted on the screen, confirmed with CHOOSE.
#[derive(Resource, Default, Debug)]
struct Preview(usize);

/// The overlay the screen was opened from; Esc returns there.
#[derive(Resource, Default, Debug)]
struct ReturnTo(Overlay);

#[derive(Component)]
struct LoadoutButton(usize);

/// The Weapon name inside a `LoadoutButton`.
#[derive(Component)]
struct ButtonWeaponName;

#[derive(Component)]
struct WeaponName;

#[derive(Component)]
struct FireModeText;

#[derive(Component)]
struct StatFill(Stat);

#[derive(Component)]
struct NextSpawnNote;

/// A stat bar on the screen, normalised across all Weapons.
#[derive(Clone, Copy, Debug)]
enum Stat {
    Damage,
    FireRate,
    Range,
    Mobility,
    Accuracy,
}

impl Stat {
    const ALL: [Stat; 5] = [
        Stat::Damage,
        Stat::FireRate,
        Stat::Range,
        Stat::Mobility,
        Stat::Accuracy,
    ];

    fn label(self) -> &'static str {
        match self {
            Stat::Damage => "Damage",
            Stat::FireRate => "Fire Rate",
            Stat::Range => "Range",
            Stat::Mobility => "Mobility",
            Stat::Accuracy => "Accuracy",
        }
    }

    /// The underlying number; bigger is better.
    fn raw(self, stats: &WeaponStats) -> f32 {
        match self {
            Stat::Damage => stats.damage_near,
            Stat::FireRate => stats.rpm,
            // Where damage stops falling off: the distance the gun is built for.
            Stat::Range => stats.falloff_end,
            Stat::Mobility => stats.move_scale,
            // Aimed spread; tighter is better.
            Stat::Accuracy => -stats.ads_spread_deg,
        }
    }

    /// Bar length from `MIN_BAR` (worst Weapon) to 1 (best Weapon).
    fn bar(self, data: &GameData, stats: &WeaponStats) -> f32 {
        let (min, max) = data
            .weapons
            .values()
            .map(|weapon| self.raw(weapon))
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(v), hi.max(v))
            });
        if max - min <= f32::EPSILON {
            return 1.0;
        }
        MIN_BAR + (1.0 - MIN_BAR) * (self.raw(stats) - min) / (max - min)
    }
}

fn fire_mode_text(stats: &WeaponStats) -> &'static str {
    match stats.fire_mode {
        FireMode::Auto => "Full auto",
        FireMode::Semi if stats.bolt_action => "Bolt action",
        FireMode::Semi => "Semi-auto",
    }
}

fn open_with_key(keys: Res<ButtonInput<KeyCode>>, mut overlay: ResMut<NextState<Overlay>>) {
    if keys.just_pressed(OPEN_KEY) {
        overlay.set(Overlay::Loadout);
    }
}

fn remember_origin(
    mut transitions: MessageReader<StateTransitionEvent<Overlay>>,
    mut return_to: ResMut<ReturnTo>,
) {
    for transition in transitions.read() {
        if transition.entered == Some(Overlay::Loadout) && transition.exited != transition.entered {
            return_to.0 = transition.exited.unwrap_or_default();
        }
    }
}

fn back_on_esc(
    keys: Res<ButtonInput<KeyCode>>,
    return_to: Res<ReturnTo>,
    mut overlay: ResMut<NextState<Overlay>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        overlay.set(return_to.0);
    }
}

fn spawn_screen(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    data: Res<GameData>,
    chosen: Res<ChosenLoadout>,
    mut preview: ResMut<Preview>,
) {
    preview.0 = chosen.0.min(data.default_loadouts.len() - 1);
    let root = commands
        .spawn((
            Name::new("LoadoutScreen"),
            DespawnOnExit(Overlay::Loadout),
            ui::backdrop(),
        ))
        .id();
    commands.spawn((ui::title(&fonts, "Choose Loadout", 48.0), ChildOf(root)));

    let columns = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                column_gap: px(16.0),
                ..default()
            },
            ChildOf(root),
        ))
        .id();

    let list = commands.spawn((ui::panel(), ChildOf(columns))).id();
    for (index, loadout) in data.default_loadouts.iter().enumerate() {
        let weapon = &data.weapon(loadout.primary).name;
        commands
            .spawn((
                ui::button(&fonts, &loadout.name),
                LoadoutButton(index),
                ChildOf(list),
            ))
            .with_child((
                ButtonWeaponName,
                ui::label(&fonts, weapon, 18.0, ui::TEXT_DIM),
                Node {
                    margin: UiRect::left(px(28.0)),
                    ..default()
                },
            ))
            .observe(move |_: On<Activate>, mut preview: ResMut<Preview>| {
                preview.0 = index;
            });
    }

    let details = commands
        .spawn((ui::panel(), ChildOf(columns)))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(24.0)),
            row_gap: px(12.0),
            border: UiRect::all(px(1.0)),
            min_width: px(440.0),
            ..default()
        })
        .id();
    commands.spawn((WeaponName, ui::title(&fonts, "", 34.0), ChildOf(details)));
    commands.spawn((
        FireModeText,
        ui::label(&fonts, "", 20.0, ui::TEXT_DIM),
        ChildOf(details),
    ));
    for stat in Stat::ALL {
        commands.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(16.0),
                ..default()
            },
            ChildOf(details),
            children![
                (
                    ui::label(&fonts, &stat.label().to_uppercase(), 20.0, ui::TEXT),
                    Node {
                        width: px(110.0),
                        ..default()
                    },
                ),
                (
                    Node {
                        width: px(BAR_WIDTH),
                        height: px(BAR_HEIGHT),
                        ..default()
                    },
                    BackgroundColor(BAR_TRACK),
                    children![(
                        StatFill(stat),
                        Node {
                            height: percent(100.0),
                            width: percent(0.0),
                            ..default()
                        },
                        BackgroundColor(ui::ACCENT),
                    )],
                ),
            ],
        ));
    }

    commands.spawn((
        NextSpawnNote,
        ui::label(&fonts, "Applies on next spawn", 22.0, ui::ACCENT),
        Visibility::Hidden,
        ChildOf(root),
    ));

    let actions = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(16.0),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    commands
        .spawn((ui::button(&fonts, "Back"), ChildOf(actions)))
        .observe(
            |_: On<Activate>, return_to: Res<ReturnTo>, mut overlay: ResMut<NextState<Overlay>>| {
                overlay.set(return_to.0);
            },
        );
    commands
        .spawn((ui::button(&fonts, "Choose"), ChildOf(actions)))
        .observe(choose);
}

/// Confirms the previewed Loadout, saves it with the settings and returns to the game.
fn choose(
    _: On<Activate>,
    preview: Res<Preview>,
    data: Res<GameData>,
    mut chosen: ResMut<ChosenLoadout>,
    settings: Option<ResMut<Settings>>,
    mut overlay: ResMut<NextState<Overlay>>,
) {
    chosen.set_if_neq(ChosenLoadout(preview.0));
    if let Some(mut settings) = settings
        && settings.loadout != preview.0
    {
        settings.loadout = preview.0;
    }
    info!("Loadout chosen: {}", chosen.get(&data).name);
    overlay.set(Overlay::None);
}

/// Highlights the previewed Loadout and shows its Weapon, stats and whether it applies now.
#[allow(clippy::too_many_arguments)]
fn refresh(
    time: Res<Time>,
    data: Res<GameData>,
    preview: Res<Preview>,
    clock: Res<SpawnClock>,
    life: Res<LocalLife>,
    held: Query<&HeldWeapon, With<LocalPlayer>>,
    buttons: Query<(&LoadoutButton, &Children)>,
    mut texts: Query<(&mut TextColor, Has<ButtonWeaponName>)>,
    mut name: Single<&mut Text, (With<WeaponName>, Without<FireModeText>)>,
    mut fire_mode: Single<&mut Text, (With<FireModeText>, Without<WeaponName>)>,
    mut fills: Query<(&StatFill, &mut Node)>,
    mut note: Single<&mut Visibility, With<NextSpawnNote>>,
) {
    let loadout = &data.default_loadouts[preview.0];
    let stats = data.weapon(loadout.primary);

    for (button, children) in &buttons {
        let selected = button.0 == preview.0;
        for child in children {
            if let Ok((mut color, weapon_name)) = texts.get_mut(*child) {
                let wanted = match (selected, weapon_name) {
                    (true, _) => ui::ACCENT,
                    (false, true) => ui::TEXT_DIM,
                    (false, false) => ui::TEXT,
                };
                color.set_if_neq(TextColor(wanted));
            }
        }
    }

    let title = stats.name.to_uppercase();
    if name.0 != title {
        name.0 = title;
    }
    let mode = fire_mode_text(stats);
    if fire_mode.0 != mode {
        fire_mode.0 = mode.to_owned();
    }
    for (fill, mut node) in &mut fills {
        let width = percent(fill.0.bar(&data, stats) * 100.0);
        if node.width != width {
            node.width = width;
        }
    }

    let equipped = held
        .single()
        .is_ok_and(|weapon| weapon.id() == loadout.primary);
    let deferred = !equipped && !clock.swap_open(time.elapsed_secs_f64(), &life, &data);
    note.set_if_neq(if deferred {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}
