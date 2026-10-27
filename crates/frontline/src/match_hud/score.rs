//! Top-centre score: `RANGERS [emblem][12] [m:ss] [7][emblem] BRIGADE`, each side with its
//! Faction's short name and emblem, coloured friendly/enemy.

use bevy::prelude::*;

use super::{ENEMY_COLOR, FRIENDLY_COLOR, MatchQuery, clock, own_side};
use crate::data::GameData;
use crate::flow::AppState;
use crate::maps::{MapId, MapRegistry};
use crate::player::LocalLife;
use crate::shared::Team;
use crate::ui::{self, UiFonts};

const FONT_SIZE: f32 = 24.0;
const BOX_WIDTH: f32 = 56.0;
const EMBLEM_SIZE: f32 = 30.0;
const SHORT_NAME_SIZE: f32 = 18.0;
const BOX_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);

#[derive(Component)]
pub(super) struct ScoreRoot;

/// Which part of the score row a text shows.
#[derive(Component, Clone, Copy)]
pub(super) enum ScorePart {
    Friendly,
    Clock,
    Enemy,
}

/// The Faction emblem and short name beside the friendly (`enemy: false`) or enemy score.
#[derive(Component, Clone, Copy)]
pub(super) struct ScoreFaction {
    enemy: bool,
}

pub(super) fn spawn_score(mut commands: Commands, fonts: Res<UiFonts>) {
    let part = |part: ScorePart, color: Color| {
        (
            part,
            Node {
                min_width: px(BOX_WIDTH),
                padding: UiRect::axes(px(8.0), px(0.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BOX_BG),
            Text::default(),
            TextFont {
                font: fonts.bold.clone().into(),
                font_size: FontSize::Px(FONT_SIZE),
                ..default()
            },
            TextColor(color),
            TextLayout::justify(Justify::Center),
        )
    };
    let emblem = |enemy: bool| {
        (
            ScoreFaction { enemy },
            ImageNode::default(),
            Node {
                width: px(EMBLEM_SIZE),
                height: px(EMBLEM_SIZE),
                ..default()
            },
        )
    };
    let short_name = |enemy: bool, color: Color| {
        (
            ScoreFaction { enemy },
            Node {
                padding: UiRect::axes(px(8.0), px(2.0)),
                ..default()
            },
            BackgroundColor(BOX_BG),
            ui::label(&fonts, "", SHORT_NAME_SIZE, color),
        )
    };
    commands.spawn((
        Name::new("Match score"),
        ScoreRoot,
        DespawnOnExit(AppState::InGame),
        Node {
            position_type: PositionType::Absolute,
            top: px(8.0),
            width: percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: px(4.0),
            ..default()
        },
        Visibility::Hidden,
        children![
            short_name(false, FRIENDLY_COLOR),
            emblem(false),
            part(ScorePart::Friendly, FRIENDLY_COLOR),
            part(ScorePart::Clock, Color::WHITE),
            part(ScorePart::Enemy, ENEMY_COLOR),
            emblem(true),
            short_name(true, ENEMY_COLOR),
        ],
    ));
}

pub(super) fn update_score(
    state: MatchQuery,
    life: Res<LocalLife>,
    data: Res<GameData>,
    maps: Res<MapRegistry>,
    assets: Res<AssetServer>,
    mut root: Single<&mut Visibility, With<ScoreRoot>>,
    mut parts: Query<(&ScorePart, &mut Text)>,
    mut emblems: Query<(&ScoreFaction, &mut ImageNode)>,
    mut short_names: Query<(&ScoreFaction, &mut Text), Without<ScorePart>>,
    fresh: Query<(), Added<ScoreFaction>>,
    mut shown: Local<Option<(MapId, Team)>>,
) {
    let Ok(state) = state.single() else {
        root.set_if_neq(Visibility::Hidden);
        *shown = None;
        return;
    };
    root.set_if_neq(Visibility::Inherited);
    let ours = own_side(life.team());
    for (part, mut text) in &mut parts {
        let value = match part {
            ScorePart::Friendly => state.scores[ours.index()].to_string(),
            ScorePart::Clock => clock(state.seconds_left),
            ScorePart::Enemy => state.scores[ours.other().index()].to_string(),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    // Factions follow the Map and our side; a new session's row starts blank.
    let key = (state.map.clone(), ours);
    if shown.as_ref() == Some(&key) && fresh.is_empty() {
        return;
    }
    *shown = Some(key);
    let faction = |ScoreFaction { enemy }: &ScoreFaction| {
        data.faction(&maps, &state.map, if *enemy { ours.other() } else { ours })
    };
    for (side, mut image) in &mut emblems {
        image.image = assets.load(faction(side).icon.clone());
    }
    for (side, mut text) in &mut short_names {
        text.0 = faction(side).short_name.to_uppercase();
    }
}
