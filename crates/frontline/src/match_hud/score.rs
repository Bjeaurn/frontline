//! Top-centre score: `[friendly score] [m:ss] [enemy score]`.

use bevy::prelude::*;

use super::{ENEMY_COLOR, FRIENDLY_COLOR, MatchQuery, clock, own_side};
use crate::player::LocalLife;

const FONT_SIZE: f32 = 20.0;
const BOX_WIDTH: f32 = 56.0;
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

pub(super) fn spawn_score(mut commands: Commands) {
    let part = |part: ScorePart, color: Color, background: Color| {
        (
            part,
            Node {
                min_width: px(BOX_WIDTH),
                padding: UiRect::axes(px(8.0), px(2.0)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(background),
            Text::default(),
            TextFont {
                font_size: FontSize::Px(FONT_SIZE),
                ..default()
            },
            TextColor(color),
            TextLayout::justify(Justify::Center),
        )
    };
    commands.spawn((
        Name::new("Match score"),
        ScoreRoot,
        Node {
            position_type: PositionType::Absolute,
            top: px(8.0),
            width: percent(100.0),
            justify_content: JustifyContent::Center,
            column_gap: px(4.0),
            ..default()
        },
        Visibility::Hidden,
        children![
            part(ScorePart::Friendly, FRIENDLY_COLOR, BOX_BG),
            part(ScorePart::Clock, Color::WHITE, BOX_BG),
            part(ScorePart::Enemy, ENEMY_COLOR, BOX_BG),
        ],
    ));
}

pub(super) fn update_score(
    state: MatchQuery,
    life: Res<LocalLife>,
    mut root: Single<&mut Visibility, With<ScoreRoot>>,
    mut parts: Query<(&ScorePart, &mut Text)>,
) {
    let Ok(state) = state.single() else {
        root.set_if_neq(Visibility::Hidden);
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
}
