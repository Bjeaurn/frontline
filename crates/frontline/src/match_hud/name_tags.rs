//! Name tags above friendly players' heads. Enemies get none (CoD-style).

use bevy::prelude::*;

use super::{FRIENDLY_COLOR, is_friendly};
use crate::data::ActiveRules;
use crate::flow::AppState;
use crate::player::{EyeCamera, LocalLife};
use crate::shared::{PlayerName, PlayerPose, PlayerTeam, RemotePlayer};

const FONT_SIZE: f32 = 14.0;
/// Width of the box the name is centred in, in logical pixels.
const TAG_WIDTH: f32 = 200.0;
/// Height above the feet the tag hangs at, in metres.
const TAG_HEIGHT: f32 = 2.1;
/// Friendlies further away than this get no tag, in metres.
const MAX_DISTANCE: f32 = 45.0;

/// UI text that follows `target` on screen.
#[derive(Component)]
pub(super) struct NameTag {
    target: Entity,
}

/// Marks a remote player that already has a `NameTag`.
#[derive(Component)]
pub(super) struct Tagged;

pub(super) fn attach_name_tags(
    fonts: Res<crate::ui::UiFonts>,
    players: Query<Entity, (With<RemotePlayer>, With<PlayerName>, Without<Tagged>)>,
    mut commands: Commands,
) {
    for target in &players {
        commands.entity(target).insert(Tagged);
        commands.spawn((
            Name::new("Name tag"),
            NameTag { target },
            DespawnOnExit(AppState::InGame),
            Node {
                position_type: PositionType::Absolute,
                width: px(TAG_WIDTH),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Text::default(),
            TextFont {
                font: fonts.bold.clone().into(),
                font_size: FontSize::Px(FONT_SIZE),
                ..default()
            },
            TextShadow {
                offset: Vec2::splat(1.5),
                color: Color::srgba(0.0, 0.0, 0.0, 0.85),
            },
            TextColor(FRIENDLY_COLOR),
            TextLayout::justify(Justify::Center),
            Visibility::Hidden,
        ));
    }
}

/// Shows each tag over its friendly, on-screen player while that player is drawn (so not while
/// dead or not yet at its spawn), if the Match Rules show name tags; despawns tags whose
/// player left.
pub(super) fn place_name_tags(
    rules: ActiveRules,
    life: Res<LocalLife>,
    camera: Query<(&Camera, &GlobalTransform), With<EyeCamera>>,
    players: Query<
        (&PlayerName, &PlayerPose, &PlayerTeam, Option<&Visibility>),
        With<RemotePlayer>,
    >,
    mut tags: Query<
        (Entity, &NameTag, &mut Node, &mut Text, &mut Visibility),
        Without<RemotePlayer>,
    >,
    mut commands: Commands,
) {
    let Ok((camera, eye)) = camera.single() else {
        return;
    };
    let enabled = rules.get().hud.name_tags;
    for (tag, NameTag { target }, mut node, mut text, mut visibility) in &mut tags {
        let Ok((name, pose, team, drawn)) = players.get(*target) else {
            commands.entity(tag).despawn();
            continue;
        };
        let head = pose.position + Vec3::Y * TAG_HEIGHT;
        let shown = enabled
            && is_friendly(life.team(), team.0)
            && drawn.is_some_and(|drawn| *drawn != Visibility::Hidden)
            && eye.translation().distance(head) <= MAX_DISTANCE;
        let screen = shown
            .then(|| camera.world_to_viewport(eye, head).ok())
            .flatten();
        let Some(screen) = screen else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        node.left = px(screen.x - TAG_WIDTH / 2.0);
        node.top = px(screen.y - FONT_SIZE);
        if text.0 != name.0 {
            text.0.clone_from(&name.0);
        }
    }
}
