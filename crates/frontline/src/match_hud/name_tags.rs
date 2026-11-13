//! Name tags above friendly players' heads. Enemies get none (CoD-style).

use bevy::prelude::*;

use super::{FRIENDLY_COLOR, is_friendly};
use crate::data::ActiveRules;
use crate::flow::AppState;
use crate::player::{EyeCamera, Killcam, LocalLife};
use crate::shared::{PlayerName, PlayerPose, PlayerTeam, RemotePlayer};

const FONT_SIZE: f32 = 14.0;
/// Width of the box the name is centred in, in logical pixels.
const TAG_WIDTH: f32 = 200.0;
/// Height above the feet the tag hangs at, in metres.
const TAG_HEIGHT: f32 = 2.1;
/// Opacity of a tag's text shadow when the tag is fully visible.
const SHADOW_ALPHA: f32 = 0.85;
/// Share of the range the tag stays fully opaque for; it fades out over the rest.
const FADE_START: f32 = 0.7;

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
                color: Color::srgba(0.0, 0.0, 0.0, SHADOW_ALPHA),
            },
            TextColor(FRIENDLY_COLOR),
            TextLayout::justify(Justify::Center),
            Visibility::Hidden,
        ));
    }
}

/// Opacity of a tag `distance` metres away given the Match Rules' name tag `range`: fully
/// opaque up to 70% of the range, fading out linearly to nothing at the range, nothing beyond
/// it (or for a range that is not positive).
fn tag_alpha(distance: f32, range: f32) -> f32 {
    if range.is_nan() || range <= 0.0 || distance.is_nan() {
        return 0.0;
    }
    let start = range * FADE_START;
    ((range - distance) / (range - start)).clamp(0.0, 1.0)
}

/// Shows each tag over its friendly, on-screen player while that player is drawn (so not while
/// dead or not yet at its spawn, nor during a Kill-cam), if the Match Rules show name tags,
/// fading it out towards the Match Rules' name tag range.
/// A player whose pose or Team has not arrived (yet, or again) only hides its tag: the tag is
/// despawned when the player itself is gone, since a player is tagged once.
pub(super) fn place_name_tags(
    rules: ActiveRules,
    life: Res<LocalLife>,
    killcam: Res<Killcam>,
    camera: Query<(&Camera, &GlobalTransform), With<EyeCamera>>,
    players: Query<
        (&PlayerName, &PlayerPose, &PlayerTeam, Option<&Visibility>),
        With<RemotePlayer>,
    >,
    tagged: Query<(), With<Tagged>>,
    mut tags: Query<
        (
            Entity,
            &NameTag,
            &mut Node,
            &mut Text,
            &mut TextColor,
            &mut TextShadow,
            &mut Visibility,
        ),
        Without<RemotePlayer>,
    >,
    mut commands: Commands,
) {
    let Ok((camera, eye)) = camera.single() else {
        return;
    };
    let (enabled, range) = {
        let rules = rules.get();
        (
            rules.hud.name_tags && !killcam.is_playing(),
            rules.hud.name_tag_range_m,
        )
    };
    for (tag, NameTag { target }, mut node, mut text, mut color, mut shadow, mut visibility) in
        &mut tags
    {
        let Ok((name, pose, team, drawn)) = players.get(*target) else {
            if tagged.contains(*target) {
                visibility.set_if_neq(Visibility::Hidden);
            } else {
                commands.entity(tag).despawn();
            }
            continue;
        };
        let head = pose.position + Vec3::Y * TAG_HEIGHT;
        let shown = enabled
            && is_friendly(life.team(), team.0)
            && drawn.is_some_and(|drawn| *drawn != Visibility::Hidden);
        let alpha = if shown {
            tag_alpha(eye.translation().distance(head), range)
        } else {
            0.0
        };
        let screen = (alpha > 0.0)
            .then(|| camera.world_to_viewport(eye, head).ok())
            .flatten();
        let Some(screen) = screen else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        let text_alpha = FRIENDLY_COLOR.alpha() * alpha;
        if color.0.alpha() != text_alpha {
            color.0.set_alpha(text_alpha);
        }
        let shadow_alpha = SHADOW_ALPHA * alpha;
        if shadow.color.alpha() != shadow_alpha {
            shadow.color.set_alpha(shadow_alpha);
        }
        node.left = px(screen.x - TAG_WIDTH / 2.0);
        node.top = px(screen.y - FONT_SIZE);
        if text.0 != name.0 {
            text.0.clone_from(&name.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_fades_from_seventy_percent_of_the_range_to_nothing_at_it() {
        assert_eq!(tag_alpha(0.0, 45.0), 1.0);
        assert_eq!(tag_alpha(10.0, 45.0), 1.0);
        assert_eq!(tag_alpha(31.0, 100.0), 1.0);
        assert_eq!(tag_alpha(100.0 * FADE_START, 100.0), 1.0);
        assert!((tag_alpha(85.0, 100.0) - 0.5).abs() < 1e-5);
        assert!((tag_alpha(77.5, 100.0) - 0.75).abs() < 1e-5);
        assert_eq!(tag_alpha(100.0, 100.0), 0.0);
        assert_eq!(tag_alpha(250.0, 100.0), 0.0);
    }

    #[test]
    fn no_range_means_no_tags() {
        assert_eq!(tag_alpha(0.0, 0.0), 0.0);
        assert_eq!(tag_alpha(5.0, -10.0), 0.0);
        assert_eq!(tag_alpha(5.0, f32::NAN), 0.0);
    }
}
