//! Name tags above friendly players' heads. Enemies get none (CoD-style).

use bevy::prelude::*;

use super::{FRIENDLY_COLOR, is_friendly};
use crate::flow::AppState;
use crate::player::{EyeCamera, LocalLife};
use crate::shared::{Health, PlayerName, PlayerPose, PlayerTeam, RemotePlayer};

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
                font_size: FontSize::Px(FONT_SIZE),
                ..default()
            },
            TextColor(FRIENDLY_COLOR),
            TextLayout::justify(Justify::Center),
            Visibility::Hidden,
        ));
    }
}

/// Shows each tag over its friendly, living, on-screen player; despawns tags whose player left.
pub(super) fn place_name_tags(
    life: Res<LocalLife>,
    camera: Query<(&Camera, &GlobalTransform), With<EyeCamera>>,
    players: Query<(&PlayerName, &PlayerPose, &PlayerTeam, Option<&Health>), With<RemotePlayer>>,
    mut tags: Query<(Entity, &NameTag, &mut Node, &mut Text, &mut Visibility)>,
    mut commands: Commands,
) {
    let Ok((camera, eye)) = camera.single() else {
        return;
    };
    for (tag, NameTag { target }, mut node, mut text, mut visibility) in &mut tags {
        let Ok((name, pose, team, health)) = players.get(*target) else {
            commands.entity(tag).despawn();
            continue;
        };
        let head = pose.position + Vec3::Y * TAG_HEIGHT;
        let shown = is_friendly(life.team(), team.0)
            && !health.is_some_and(|health| health.is_dead())
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
