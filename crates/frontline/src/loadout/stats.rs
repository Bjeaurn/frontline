//! The stat bars of a Weapon, shared by the Loadout screen and the Create a Class editor. Each
//! bar compares the Weapon (with its Attachments) to the best of every Weapon.

use bevy::prelude::*;

use crate::data::{FireMode, GameData, WeaponStats};
use crate::ui::{self, UiFonts};

/// Shortest stat bar, so the weakest Weapon still shows a sliver.
const MIN_BAR: f32 = 0.12;
const BAR_WIDTH: f32 = 260.0;
const BAR_HEIGHT: f32 = 10.0;
const BAR_TRACK: Color = Color::srgba(1.0, 1.0, 1.0, 0.12);

/// The filled part of a stat bar.
#[derive(Component)]
pub(super) struct StatFill(Stat);

#[derive(Clone, Copy, Debug)]
enum Stat {
    Damage,
    FireRate,
    Range,
    Mobility,
    Accuracy,
    Handling,
}

impl Stat {
    const ALL: [Stat; 6] = [
        Stat::Damage,
        Stat::FireRate,
        Stat::Range,
        Stat::Mobility,
        Stat::Accuracy,
        Stat::Handling,
    ];

    fn label(self) -> &'static str {
        match self {
            Stat::Damage => "Damage",
            Stat::FireRate => "Fire Rate",
            Stat::Range => "Range",
            Stat::Mobility => "Mobility",
            Stat::Accuracy => "Accuracy",
            Stat::Handling => "Handling",
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
            Stat::Accuracy => 1.0 / (1.0 + stats.ads_spread_deg),
            // How quickly the sights come up.
            Stat::Handling => 1.0 / stats.ads_secs.max(f32::EPSILON),
        }
    }

    /// Bar length from `MIN_BAR` up to 1 (as good as the best Weapon).
    fn bar(self, data: &GameData, stats: &WeaponStats) -> f32 {
        let best = data
            .weapons
            .values()
            .map(|weapon| self.raw(weapon))
            .fold(self.raw(stats), f32::max);
        (self.raw(stats) / best).clamp(MIN_BAR, 1.0)
    }
}

pub(super) fn fire_mode_text(stats: &WeaponStats) -> &'static str {
    match stats.fire_mode {
        FireMode::Auto => "Full auto",
        FireMode::Semi if stats.bolt_action => "Bolt action",
        FireMode::Semi => "Semi-auto",
    }
}

/// Spawns one labelled bar per stat under `parent`.
pub(super) fn spawn_bars(commands: &mut Commands, fonts: &UiFonts, parent: Entity) {
    for stat in Stat::ALL {
        commands.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(16.0),
                ..default()
            },
            ChildOf(parent),
            children![
                (
                    ui::label(fonts, &stat.label().to_uppercase(), 20.0, ui::TEXT),
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
}

/// Sets every bar to `stats`.
pub(super) fn update_bars(
    fills: &mut Query<(&StatFill, &mut Node)>,
    data: &GameData,
    stats: &WeaponStats,
) {
    for (fill, mut node) in fills {
        let width = percent(fill.0.bar(data, stats) * 100.0);
        if node.width != width {
            node.width = width;
        }
    }
}
