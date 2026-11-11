//! The soldier drawn for remote players and their bodies: one skinned glTF model
//! (`assets/models/characters/soldier.glb`, built by `tools/assets/build_soldier.py`) with a
//! helmet or cap, a plate carrier with pouches, a pack, boots and gloves, holding a rifle.
//!
//! The model has no animation clips. `pose` works out, every frame, how far each bone is turned
//! from its rest pose for a stance, a walk cycle and the aim pitch, and `Rest::locals` turns that
//! into the bones' local transforms; the skin does the rest, so knees and elbows bend without
//! gaps. The rest pose is the soldier standing with the rifle at the shoulder: the arms never
//! move relative to the `shoulders` bone, which pitches with the aim and carries the rifle.
//!
//! A Faction's look is the model's materials with other tints (`Slot`), headgear nodes shown or
//! hidden, and a balaclava and goggles when the look asks for them. Team colour is the `team`
//! material (chest and back patches, armbands) in the friendly or enemy colour. Nothing here
//! touches hit zones: it is all visuals.
//!
//! Local space: feet at the origin, facing -Z, +X to the soldier's right. Bones, nodes and
//! materials by name:
//!
//! - bones: `hips` (the root, at the hip joints' height), `spine`, `chest`, `neck`, `head`;
//!   `shoulders` (pivot between the shoulders: arms and rifle hang from it); `upper_arm_{r,l}`,
//!   `forearm_{r,l}`, `hand_{r,l}`; `thigh_{r,l}`, `shin_{r,l}`, `foot_{r,l}`
//! - nodes: `gun_mount` (child of `shoulders`: where the rifle's origin is), `headgear_<kind>` for
//!   each `Headgear`, `mask` (balaclava), `goggles`, and the body meshes
//! - materials: `uniform`, `gear`, `skin`, `headgear` and `mask` (tinted per Faction), `fixed`
//!   (boots, gloves, straps: as the model has them) and `team`

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::asset::AssetPath;
use bevy::ecs::system::SystemParam;
use bevy::gltf::GltfMaterialName;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldInstanceReady};

use crate::assets::EMBEDDED_ROOT;
use crate::data::{GameData, GunModel, Headgear, SoldierLook};
use crate::shared::Stance;
use crate::weapon::rig::gun_rig;

/// Embedded model file, under the assets folder.
pub(super) const MODEL: &str = "models/characters/soldier.glb";

/// The bones of the model, parents before children (`BONES` is in this order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bone {
    Hips,
    Spine,
    Chest,
    Neck,
    Head,
    /// Pivot between the shoulders: arms and rifle pitch about it with the aim.
    Shoulders,
    UpperArmR,
    ForearmR,
    HandR,
    UpperArmL,
    ForearmL,
    HandL,
    ThighR,
    ShinR,
    FootR,
    ThighL,
    ShinL,
    FootL,
}

pub(super) const BONE_COUNT: usize = 18;

pub(super) const BONES: [Bone; BONE_COUNT] = [
    Bone::Hips,
    Bone::Spine,
    Bone::Chest,
    Bone::Neck,
    Bone::Head,
    Bone::Shoulders,
    Bone::UpperArmR,
    Bone::ForearmR,
    Bone::HandR,
    Bone::UpperArmL,
    Bone::ForearmL,
    Bone::HandL,
    Bone::ThighR,
    Bone::ShinR,
    Bone::FootR,
    Bone::ThighL,
    Bone::ShinL,
    Bone::FootL,
];

impl Bone {
    /// The node name in the model.
    pub(super) fn name(self) -> &'static str {
        match self {
            Bone::Hips => "hips",
            Bone::Spine => "spine",
            Bone::Chest => "chest",
            Bone::Neck => "neck",
            Bone::Head => "head",
            Bone::Shoulders => "shoulders",
            Bone::UpperArmR => "upper_arm_r",
            Bone::ForearmR => "forearm_r",
            Bone::HandR => "hand_r",
            Bone::UpperArmL => "upper_arm_l",
            Bone::ForearmL => "forearm_l",
            Bone::HandL => "hand_l",
            Bone::ThighR => "thigh_r",
            Bone::ShinR => "shin_r",
            Bone::FootR => "foot_r",
            Bone::ThighL => "thigh_l",
            Bone::ShinL => "shin_l",
            Bone::FootL => "foot_l",
        }
    }

    pub(super) fn parent(self) -> Option<Bone> {
        match self {
            Bone::Hips => None,
            Bone::Spine | Bone::ThighR | Bone::ThighL => Some(Bone::Hips),
            Bone::Chest => Some(Bone::Spine),
            Bone::Neck | Bone::Shoulders => Some(Bone::Chest),
            Bone::Head => Some(Bone::Neck),
            Bone::UpperArmR | Bone::UpperArmL => Some(Bone::Shoulders),
            Bone::ForearmR => Some(Bone::UpperArmR),
            Bone::HandR => Some(Bone::ForearmR),
            Bone::ForearmL => Some(Bone::UpperArmL),
            Bone::HandL => Some(Bone::ForearmL),
            Bone::ShinR => Some(Bone::ThighR),
            Bone::FootR => Some(Bone::ShinR),
            Bone::ShinL => Some(Bone::ThighL),
            Bone::FootL => Some(Bone::ShinL),
        }
    }
}

/// Hip joint height standing, in metres.
const STAND_HIP: f32 = 0.93;
const CROUCH_HIP: Vec3 = Vec3::new(0.0, 0.44, 0.15);
const PRONE_HIP: Vec3 = Vec3::new(0.0, 0.14, 0.62);
/// Metres of ground covered by one full walk cycle (both legs), so steps match footsteps.
const STRIDE_LENGTH: f32 = 2.2;
/// Crouched: thighs near horizontal, shins slanting back to the heel.
const CROUCH_THIGH: f32 = 1.48;
const CROUCH_KNEE: f32 = -2.11;
const CROUCH_LEAN: f32 = -0.45;
/// How much of the torso's forward lean each of the hips, spine and chest takes (cumulative: the
/// chest ends up at the whole lean).
const LEAN_SPLIT: [f32; 3] = [0.3, 0.7, 1.0];

/// How far into its walk cycle a soldier is and how hard it moves.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Gait {
    /// Radians into the cycle.
    pub phase: f32,
    /// 0 standing still to 1 at walking speed.
    pub walk: f32,
    /// 0 at walking speed to 1 at sprinting speed.
    pub run: f32,
}

impl Gait {
    /// Moves the cycle on `dt` seconds at horizontal `speed`, given the walking and sprinting
    /// speeds of the movement data.
    pub fn advance(&mut self, speed: f32, walk_speed: f32, sprint_speed: f32, dt: f32) {
        self.walk = (speed / walk_speed.max(f32::EPSILON)).clamp(0.0, 1.0);
        self.run =
            ((speed - walk_speed) / (sprint_speed - walk_speed).max(f32::EPSILON)).clamp(0.0, 1.0);
        self.phase = (self.phase + TAU * speed / STRIDE_LENGTH * dt) % TAU;
    }
}

/// How a soldier stands: where the hips are, and how far every bone is turned from its rest pose.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pose {
    /// The hips' position relative to the feet.
    hips: Vec3,
    /// Extra height of the `shoulders` pivot over where the spine carries it (a prone soldier
    /// props the rifle on its elbows).
    shoulders_lift: Vec3,
    /// Each bone's rotation relative to its rest pose in feet space (not relative to its parent),
    /// indexed like `BONES`.
    delta: [Quat; BONE_COUNT],
}

/// The pose of a soldier in `stance` mid-`gait`, aiming `pitch` radians up.
pub(super) fn pose(stance: Stance, gait: Gait, pitch: f32) -> Pose {
    let pitch = pitch.clamp(-1.3, 1.3);
    let w = gait.walk;
    let r = gait.run;
    let phase = gait.phase;
    let rot_x = Quat::from_rotation_x;
    // Per side: +1 left, -1 right; opposite legs are half a cycle apart.
    let sin = |side: f32| side * phase.sin();
    let cos = |side: f32| side * phase.cos();

    // Per leg (right, left): the thigh's pitch and splay, the knee bend, the foot's turn.
    let mut legs = [(0.0, 0.0, 0.0, 0.0); 2];
    let (hips, lean, head, aim, shoulders_lift) = match stance {
        Stance::Standing => {
            let hip = Vec3::new(0.0, STAND_HIP + 0.018 * w * (2.0 * phase).cos(), 0.0);
            let swing = (0.45 + 0.3 * r) * w;
            let knee = (0.8 + 0.5 * r) * w;
            for (leg, side) in legs.iter_mut().zip([-1.0, 1.0]) {
                let thigh = swing * sin(side);
                let bend = -knee * cos(side).max(0.0) - 0.04 * w;
                // Toes up as the foot reaches forward, down as it trails.
                *leg = (thigh, 0.0, bend, 0.5 * (thigh + bend));
            }
            (
                hip,
                LEAN_SPLIT.map(|share| -(0.03 * w + 0.2 * r) * share),
                pitch * 0.8,
                pitch - 0.55 * r,
                Vec3::ZERO,
            )
        }
        Stance::Crouching => {
            let hip = CROUCH_HIP + Vec3::Y * 0.01 * w * (2.0 * phase).cos();
            for (leg, side) in legs.iter_mut().zip([-1.0, 1.0]) {
                let thigh = CROUCH_THIGH + 0.25 * w * sin(side);
                let bend = CROUCH_KNEE - 0.15 * w * cos(side).max(0.0);
                *leg = (thigh, 0.0, bend, 0.15 * w * sin(side));
            }
            (
                hip,
                LEAN_SPLIT.map(|share| (CROUCH_LEAN - 0.05 * w) * share),
                pitch * 0.8,
                pitch,
                Vec3::ZERO,
            )
        }
        Stance::Prone => {
            for (leg, side) in legs.iter_mut().zip([-1.0, 1.0]) {
                *leg = (
                    -FRAC_PI_2 + 0.1 * w * sin(side),
                    0.07 * side,
                    0.0,
                    -FRAC_PI_2,
                );
            }
            (
                PRONE_HIP,
                [-FRAC_PI_2, -1.5, -1.3],
                pitch.clamp(-0.5, 0.9),
                pitch.clamp(-0.3, 0.5),
                Vec3::Y * 0.1,
            )
        }
    };
    let mut delta = [Quat::IDENTITY; BONE_COUNT];
    let set = |delta: &mut [Quat; BONE_COUNT], bone: Bone, rotation: Quat| {
        delta[bone as usize] = rotation;
    };
    set(&mut delta, Bone::Hips, rot_x(lean[0]));
    set(&mut delta, Bone::Spine, rot_x(lean[1]));
    set(&mut delta, Bone::Chest, rot_x(lean[2]));
    set(&mut delta, Bone::Neck, rot_x((lean[2] + head) / 2.0));
    set(&mut delta, Bone::Head, rot_x(head));
    for bone in [
        Bone::Shoulders,
        Bone::UpperArmR,
        Bone::ForearmR,
        Bone::HandR,
        Bone::UpperArmL,
        Bone::ForearmL,
        Bone::HandL,
    ] {
        set(&mut delta, bone, rot_x(aim));
    }
    for (side, [thigh, shin, foot]) in [
        [Bone::ThighR, Bone::ShinR, Bone::FootR],
        [Bone::ThighL, Bone::ShinL, Bone::FootL],
    ]
    .into_iter()
    .enumerate()
    {
        let (pitch, yaw, knee, turn) = legs[side];
        let thigh_rotation = Quat::from_rotation_y(yaw) * rot_x(pitch);
        set(&mut delta, thigh, thigh_rotation);
        set(&mut delta, shin, thigh_rotation * rot_x(knee));
        set(&mut delta, foot, rot_x(turn));
    }
    Pose {
        hips,
        shoulders_lift,
        delta,
    }
}

/// Where the corpse tips about: height above the feet of the soldier's back surface, so a body
/// laid on its back rests on the ground.
pub(super) fn back_height(stance: Stance) -> f32 {
    match stance {
        Stance::Standing => 0.25,
        Stance::Crouching => 0.4,
        Stance::Prone => 0.0,
    }
}

/// The model's bones at rest, to turn a `Pose` into local transforms.
#[derive(Clone, Debug)]
pub(super) struct Rest {
    /// Local transforms of the bones in the model.
    local: [Transform; BONE_COUNT],
    /// Rotation of each bone in feet space at rest.
    world: [Quat; BONE_COUNT],
    /// Rotation, in feet space, of the node `hips` hangs from (the model's armature).
    root: Quat,
}

impl Rest {
    /// The rest pose of bones with local transforms `local`, `hips` hanging from a node turned by
    /// `root`.
    pub(super) fn new(local: [Transform; BONE_COUNT], root: Quat) -> Self {
        let mut world = [Quat::IDENTITY; BONE_COUNT];
        for (i, bone) in BONES.iter().enumerate() {
            let parent = bone.parent().map_or(root, |parent| world[parent as usize]);
            world[i] = parent * local[i].rotation;
        }
        Self { local, world, root }
    }

    /// The bones' local transforms for `pose`.
    pub(super) fn locals(&self, pose: &Pose) -> [Transform; BONE_COUNT] {
        let mut turned = [Quat::IDENTITY; BONE_COUNT];
        let mut locals = self.local;
        for (i, bone) in BONES.iter().enumerate() {
            turned[i] = pose.delta[i] * self.world[i];
            let parent = bone
                .parent()
                .map_or(self.root, |parent| turned[parent as usize]);
            locals[i].rotation = parent.inverse() * turned[i];
        }
        locals[Bone::Hips as usize].translation = self.root.inverse() * pose.hips;
        locals[Bone::Shoulders as usize].translation +=
            turned[Bone::Chest as usize].inverse() * pose.shoulders_lift;
        locals
    }
}

/// The held rifle's transform relative to the `gun_mount` node: the viewmodel's rifle, in the
/// same place in the hands as in first person.
fn gun_model() -> Transform {
    gun_rig(GunModel::Carbine).model_transform()
}

/// What a material of the model is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Slot {
    Uniform,
    Gear,
    Skin,
    Headgear,
    Mask,
    /// Boots, gloves, straps, goggles: as the model has them.
    Fixed,
    /// Patches and armbands in the team colour.
    Team,
}

const SLOT_COUNT: usize = 7;

impl Slot {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "uniform" => Slot::Uniform,
            "gear" => Slot::Gear,
            "skin" => Slot::Skin,
            "headgear" => Slot::Headgear,
            "mask" => Slot::Mask,
            "fixed" => Slot::Fixed,
            "team" => Slot::Team,
            _ => return None,
        })
    }
}

/// The model's tintable materials are a grey fabric detail map times the Faction's colour; this
/// brings the colour back up to what the Faction's look says.
const DETAIL_GAIN: f32 = 1.45;
/// A balaclava is the uniform's colour, darker.
const MASK_SHADE: f32 = 0.35;

fn tint(slot: Slot, look: &SoldierLook) -> Option<Color> {
    let srgb = |(r, g, b): (f32, f32, f32)| Color::srgb(r, g, b).to_linear();
    let linear = match slot {
        Slot::Uniform => srgb(look.uniform),
        Slot::Gear => srgb(look.vest),
        Slot::Skin => srgb(look.skin),
        Slot::Headgear => srgb(look.headgear_color),
        Slot::Mask => srgb(look.uniform) * MASK_SHADE,
        Slot::Fixed | Slot::Team => return None,
    };
    Some(Color::linear_rgb(
        linear.red * DETAIL_GAIN,
        linear.green * DETAIL_GAIN,
        linear.blue * DETAIL_GAIN,
    ))
}

/// The node of a Headgear in the model.
fn headgear_node(headgear: Headgear) -> &'static str {
    match headgear {
        Headgear::Helmet => "headgear_helmet",
        Headgear::Beret => "headgear_beret",
        Headgear::Boonie => "headgear_boonie",
        Headgear::Cap => "headgear_cap",
        Headgear::Beanie => "headgear_beanie",
        Headgear::Shemagh => "headgear_shemagh",
        Headgear::Ushanka => "headgear_ushanka",
    }
}

const HEADGEARS: [Headgear; 7] = [
    Headgear::Helmet,
    Headgear::Beret,
    Headgear::Boonie,
    Headgear::Cap,
    Headgear::Beanie,
    Headgear::Shemagh,
    Headgear::Ushanka,
];

/// The soldier model, the rifle it holds and every material made from the model's own.
#[derive(Resource)]
pub(super) struct Soldiers {
    scene: Handle<WorldAsset>,
    gun: Handle<WorldAsset>,
    /// The model's own material of each slot, as loaded (with the fabric detail map).
    base: HashMap<Slot, Handle<StandardMaterial>>,
    /// The base material in each Faction's tint.
    tinted: HashMap<(usize, Slot), Handle<StandardMaterial>>,
    friendly: Handle<StandardMaterial>,
    enemy: Handle<StandardMaterial>,
}

pub(super) fn load_soldiers(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    let scene = |file: &str| {
        asset_server.load(AssetPath::from(format!("{EMBEDDED_ROOT}{file}")).with_label("Scene0"))
    };
    commands.insert_resource(Soldiers {
        scene: scene(MODEL),
        gun: scene(gun_rig(GunModel::Carbine).file),
        base: HashMap::new(),
        tinted: HashMap::new(),
        friendly: materials.add(super::remote::team_material(super::remote::team_color(
            true,
        ))),
        enemy: materials.add(super::remote::team_material(super::remote::team_color(
            false,
        ))),
    });
}

/// A soldier whose model is still loading: what to put on it once it is there.
#[derive(Component, Debug)]
pub(super) struct Dressing {
    faction: usize,
    friendly: bool,
    pose: Pose,
    /// Materials of its own, so it can fade without its Faction's other soldiers fading with it.
    own_materials: bool,
}

/// A spawned soldier's parts, on the entity the model hangs from.
#[derive(Component, Debug)]
pub(super) struct SoldierBones {
    pub bones: [Entity; BONE_COUNT],
    pub rest: Rest,
    /// Every mesh of the model with its material slot.
    meshes: Vec<(Entity, Slot)>,
    headgear: [Entity; HEADGEARS.len()],
    mask: Entity,
    goggles: Entity,
    own_materials: bool,
}

impl SoldierBones {
    /// The entities with the model's meshes, whose materials a body with its own fades.
    pub fn mesh_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.meshes.iter().map(|&(entity, _)| entity)
    }
}

impl Soldiers {
    /// Spawns a soldier under `parent`, dressed as Faction `faction` in `pose` once its model is
    /// there. Returns the entity the model hangs from (`SoldierBones` is added to it then).
    pub fn spawn(
        &self,
        commands: &mut Commands,
        parent: Entity,
        faction: usize,
        friendly: bool,
        pose: Pose,
        own_materials: bool,
    ) -> Entity {
        commands
            .spawn((
                Name::new("Soldier"),
                WorldAssetRoot(self.scene.clone()),
                Transform::default(),
                Visibility::default(),
                Dressing {
                    faction,
                    friendly,
                    pose,
                    own_materials,
                },
                ChildOf(parent),
            ))
            .id()
    }
}

/// Dresses spawned soldiers: the material of each mesh, the headgear shown.
#[derive(SystemParam)]
pub(super) struct Dresser<'w, 's> {
    soldiers: ResMut<'w, Soldiers>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    data: Res<'w, GameData>,
    commands: Commands<'w, 's>,
}

impl Dresser<'_, '_> {
    /// The material for `slot` in Faction `faction`'s look.
    fn material(
        &mut self,
        slot: Slot,
        faction: usize,
        friendly: bool,
        own: bool,
    ) -> Option<Handle<StandardMaterial>> {
        let shared = match slot {
            Slot::Team => {
                if friendly {
                    self.soldiers.friendly.clone()
                } else {
                    self.soldiers.enemy.clone()
                }
            }
            Slot::Fixed => self.soldiers.base.get(&slot)?.clone(),
            _ => {
                let key = (faction, slot);
                if let Some(handle) = self.soldiers.tinted.get(&key) {
                    handle.clone()
                } else {
                    let look = &self.data.factions[faction.min(self.data.factions.len() - 1)].look;
                    let mut material = self.materials.get(self.soldiers.base.get(&slot)?)?.clone();
                    material.base_color = tint(slot, look)?;
                    let handle = self.materials.add(material);
                    self.soldiers.tinted.insert(key, handle.clone());
                    handle
                }
            }
        };
        if !own {
            return Some(shared);
        }
        let material = self.materials.get(&shared)?.clone();
        Some(self.materials.add(material))
    }

    /// Puts Faction `faction`'s look on the soldier, with team marks for a friend or a foe.
    pub fn dress(&mut self, bones: &SoldierBones, faction: usize, friendly: bool) {
        let mut made: [Option<Handle<StandardMaterial>>; SLOT_COUNT] = Default::default();
        for &(entity, slot) in &bones.meshes {
            let handle = match &made[slot as usize] {
                Some(handle) => Some(handle.clone()),
                None => {
                    let handle = self.material(slot, faction, friendly, bones.own_materials);
                    made[slot as usize].clone_from(&handle);
                    handle
                }
            };
            if let Some(handle) = handle {
                self.commands.entity(entity).insert(MeshMaterial3d(handle));
            }
        }
        let look = &self.data.factions[faction.min(self.data.factions.len() - 1)].look;
        let show = |on: bool| {
            if on {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            }
        };
        for (headgear, entity) in HEADGEARS.into_iter().zip(bones.headgear) {
            self.commands
                .entity(entity)
                .insert(show(headgear == look.headgear));
        }
        self.commands.entity(bones.mask).insert(show(look.mask));
        self.commands
            .entity(bones.goggles)
            .insert(show(look.goggles));
    }
}

/// A soldier's model is ready: finds its bones, meshes and nodes, dresses it, poses it and puts
/// the rifle in its hands.
#[allow(clippy::too_many_arguments)]
pub(super) fn bind_soldier(
    ready: On<WorldInstanceReady>,
    dressings: Query<&Dressing>,
    children: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    transforms: Query<&Transform>,
    meshes: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>)>,
    mut dresser: Dresser,
) {
    let root = ready.entity;
    let Ok(dressing) = dressings.get(root) else {
        return;
    };
    dresser.commands.entity(root).remove::<Dressing>();
    let mut bone_entities: [Option<Entity>; BONE_COUNT] = [None; BONE_COUNT];
    let mut headgear: [Option<Entity>; HEADGEARS.len()] = [None; HEADGEARS.len()];
    let (mut mask, mut goggles, mut mount) = (None, None, None);
    let mut mesh_slots = Vec::new();
    for entity in children.iter_descendants(root) {
        if let Ok(name) = names.get(entity) {
            let name = name.as_str();
            if let Some(i) = BONES.iter().position(|bone| bone.name() == name) {
                bone_entities[i] = Some(entity);
            } else if let Some(i) = HEADGEARS
                .iter()
                .position(|&headgear| headgear_node(headgear) == name)
            {
                headgear[i] = Some(entity);
            } else if name == "mask" {
                mask = Some(entity);
            } else if name == "goggles" {
                goggles = Some(entity);
            } else if name == "gun_mount" {
                mount = Some(entity);
            }
        }
        if let Ok((material_name, material)) = meshes.get(entity)
            && let Some(slot) = Slot::from_name(material_name)
        {
            dresser
                .soldiers
                .base
                .entry(slot)
                .or_insert_with(|| material.0.clone());
            mesh_slots.push((entity, slot));
        }
    }
    let (Some(bones), Some(headgear), Some(mask), Some(goggles), Some(mount)) = (
        bone_entities.into_iter().collect::<Option<Vec<_>>>(),
        headgear.into_iter().collect::<Option<Vec<_>>>(),
        mask,
        goggles,
        mount,
    ) else {
        error!("{MODEL} lacks bones or nodes the soldier needs; not drawing it");
        return;
    };
    let bones: [Entity; BONE_COUNT] = bones.try_into().expect("one entity per bone");
    let headgear: [Entity; HEADGEARS.len()] = headgear.try_into().expect("one per headgear");

    let local = bones.map(|bone| transforms.get(bone).copied().unwrap_or_default());
    // The nodes between the model's root and `hips` (its armature) turn the rest pose.
    let mut root_rotation = Quat::IDENTITY;
    let mut node = parents.get(bones[0]).map(ChildOf::parent).ok();
    while let Some(current) = node.filter(|&current| current != root) {
        root_rotation = transforms
            .get(current)
            .map_or(Quat::IDENTITY, |t| t.rotation)
            * root_rotation;
        node = parents.get(current).map(ChildOf::parent).ok();
    }
    let rest = Rest::new(local, root_rotation);
    for (bone, transform) in bones.iter().zip(rest.locals(&dressing.pose)) {
        dresser.commands.entity(*bone).insert(transform);
    }
    let soldier = SoldierBones {
        bones,
        rest,
        meshes: mesh_slots,
        headgear,
        mask,
        goggles,
        own_materials: dressing.own_materials,
    };
    dresser.dress(&soldier, dressing.faction, dressing.friendly);
    let gun = dresser.soldiers.gun.clone();
    dresser.commands.spawn((
        Name::new("SoldierGun"),
        WorldAssetRoot(gun),
        gun_model(),
        ChildOf(mount),
    ));
    dresser.commands.entity(root).insert(soldier);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// The shipped model's JSON chunk.
    fn model_json() -> Value {
        let path = format!("{}/assets/{MODEL}", env!("CARGO_MANIFEST_DIR"));
        let glb = std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
        assert_eq!(&glb[..4], b"glTF", "{path}");
        let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        serde_json::from_slice(&glb[20..20 + length]).expect("a JSON chunk")
    }

    fn node_named<'a>(json: &'a Value, name: &str) -> (usize, &'a Value) {
        json["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .find(|(_, node)| node["name"] == name)
            .unwrap_or_else(|| panic!("{MODEL} has no node {name}"))
    }

    fn local(node: &Value) -> Transform {
        let floats = |key: &str| -> Option<Vec<f32>> {
            node[key]
                .as_array()
                .map(|values| values.iter().map(|v| v.as_f64().unwrap() as f32).collect())
        };
        Transform {
            translation: floats("translation").map_or(Vec3::ZERO, |v| Vec3::from_slice(&v)),
            rotation: floats("rotation").map_or(Quat::IDENTITY, |v| Quat::from_slice(&v)),
            scale: floats("scale").map_or(Vec3::ONE, |v| Vec3::from_slice(&v)),
        }
    }

    #[test]
    fn the_model_has_every_bone_node_slot_and_headgear_the_code_looks_up() {
        let json = model_json();
        for bone in BONES {
            node_named(&json, bone.name());
        }
        for headgear in HEADGEARS {
            node_named(&json, headgear_node(headgear));
        }
        for name in ["mask", "goggles", "gun_mount"] {
            node_named(&json, name);
        }
        let materials: Vec<&str> = json["materials"]
            .as_array()
            .unwrap()
            .iter()
            .map(|material| material["name"].as_str().unwrap())
            .collect();
        for name in [
            "uniform", "gear", "skin", "headgear", "mask", "fixed", "team",
        ] {
            assert!(Slot::from_name(name).is_some(), "no Slot for {name}");
            assert!(materials.contains(&name), "no material {name}");
        }
        assert_eq!(SLOT_COUNT, 7);
        assert!(
            materials.iter().all(|name| Slot::from_name(name).is_some()),
            "a material no Slot knows: {materials:?}"
        );
    }

    #[test]
    fn the_bones_hang_from_the_parents_the_code_names() {
        let json = model_json();
        let nodes = json["nodes"].as_array().unwrap();
        for bone in BONES {
            let Some(parent) = bone.parent() else {
                continue;
            };
            let (_, parent_node) = node_named(&json, parent.name());
            let (index, _) = node_named(&json, bone.name());
            let children = parent_node["children"].as_array().unwrap();
            assert!(
                children
                    .iter()
                    .any(|child| child.as_u64() == Some(index as u64)),
                "{} is not a child of {} (of {} nodes)",
                bone.name(),
                parent.name(),
                nodes.len()
            );
        }
    }

    /// The model's bones at rest, from its node tree.
    fn rest() -> Rest {
        let json = model_json();
        let nodes = json["nodes"].as_array().unwrap();
        let (hips, _) = node_named(&json, Bone::Hips.name());
        // The nodes above `hips` (the armature) turn the rest pose.
        let mut root_rotation = Quat::IDENTITY;
        let mut current = hips;
        while let Some(parent) = nodes.iter().position(|node| {
            node["children"]
                .as_array()
                .is_some_and(|children| children.iter().any(|c| c.as_u64() == Some(current as u64)))
        }) {
            root_rotation = local(&nodes[parent]).rotation * root_rotation;
            current = parent;
        }
        let transforms = BONES.map(|bone| local(node_named(&json, bone.name()).1));
        Rest::new(transforms, root_rotation)
    }

    /// Where each bone's joint is, relative to the feet, with the hips' parent turned by `rest`'s
    /// root.
    fn joints(rest: &Rest, pose: &Pose) -> [Vec3; BONE_COUNT] {
        let locals = rest.locals(pose);
        let mut world = [Transform::IDENTITY; BONE_COUNT];
        for (i, bone) in BONES.iter().enumerate() {
            let parent = bone
                .parent()
                .map_or(Transform::from_rotation(rest.root), |parent| {
                    world[parent as usize]
                });
            world[i] = parent * locals[i];
        }
        world.map(|transform| transform.translation)
    }

    fn at(joints: &[Vec3; BONE_COUNT], bone: Bone) -> Vec3 {
        joints[bone as usize]
    }

    #[test]
    fn standing_still_the_feet_are_on_the_ground_under_the_hips() {
        let rest = rest();
        let joints = joints(&rest, &pose(Stance::Standing, Gait::default(), 0.0));
        for foot in [Bone::FootR, Bone::FootL] {
            let ankle = at(&joints, foot);
            assert!(
                (0.05..0.13).contains(&ankle.y),
                "{foot:?} ankle at {}",
                ankle.y
            );
        }
        let hips = at(&joints, Bone::Hips);
        assert!((hips.y - STAND_HIP).abs() < 0.01, "hips at {}", hips.y);
        let head = at(&joints, Bone::Head);
        assert!((1.5..1.7).contains(&head.y), "head joint at {}", head.y);
    }

    #[test]
    fn crouched_and_prone_the_soldier_is_lower_and_the_feet_stay_near_the_ground() {
        let rest = rest();
        let standing = joints(&rest, &pose(Stance::Standing, Gait::default(), 0.0));
        let mut last = at(&standing, Bone::Head).y;
        for stance in [Stance::Crouching, Stance::Prone] {
            let joints = joints(&rest, &pose(stance, Gait::default(), 0.0));
            let head = at(&joints, Bone::Head).y;
            assert!(head < last - 0.3, "{stance:?} head at {head}");
            last = head;
            for foot in [Bone::FootR, Bone::FootL] {
                let ankle = at(&joints, foot).y;
                assert!(
                    (-0.02..0.25).contains(&ankle),
                    "{stance:?} {foot:?} ankle at {ankle}"
                );
            }
        }
    }

    #[test]
    fn a_walking_soldier_always_has_a_foot_down_and_a_sprinting_one_never_sinks() {
        let rest = rest();
        // (run, the highest the lower ankle may be): a walk keeps one foot planted, a sprint has
        // a short flight phase where both feet are off the ground.
        for (run, highest) in [(0.0, 0.18), (1.0, 0.35)] {
            for step in 0..32 {
                let gait = Gait {
                    phase: TAU * step as f32 / 32.0,
                    walk: 1.0,
                    run,
                };
                let joints = joints(&rest, &pose(Stance::Standing, gait, 0.0));
                let lower = at(&joints, Bone::FootR).y.min(at(&joints, Bone::FootL).y);
                assert!(
                    (0.05..highest).contains(&lower),
                    "run {run} step {step}: lower ankle at {lower}"
                );
            }
        }
    }

    #[test]
    fn a_full_cycle_covers_one_stride() {
        let mut gait = Gait::default();
        let speed = 2.0;
        // Time for `STRIDE_LENGTH` metres, in 60 steps.
        let dt = STRIDE_LENGTH / speed / 60.0;
        for _ in 0..60 {
            gait.advance(speed, 2.0, 5.0, dt);
        }
        assert!(
            gait.phase < 0.01 || TAU - gait.phase < 0.01,
            "{} rad into the cycle after one stride",
            gait.phase
        );
        assert_eq!((gait.walk, gait.run), (1.0, 0.0));
        gait.advance(3.5, 2.0, 5.0, 0.0);
        assert_eq!((gait.walk, gait.run), (1.0, 0.5));
    }

    #[test]
    fn aiming_pitches_the_hands_about_the_shoulders() {
        let rest = rest();
        let joints_at_rest = joints(&rest, &pose(Stance::Standing, Gait::default(), 0.0));
        let level = joints_at_rest;
        let up = joints(&rest, &pose(Stance::Standing, Gait::default(), 0.7));
        let down = joints(&rest, &pose(Stance::Standing, Gait::default(), -0.7));
        for hand in [Bone::HandR, Bone::HandL] {
            let (level, up, down) = (at(&level, hand), at(&up, hand), at(&down, hand));
            // Aiming up lifts a hand over where aiming down puts it.
            assert!(up.y > down.y + 0.1, "{hand:?} up {up} down {down}");
            assert!(
                level.distance(up) > 0.05 && level.distance(down) > 0.05,
                "{hand:?}"
            );
            // About the pivot: the distance to the shoulders does not change.
            let pivot = at(&joints_at_rest, Bone::Shoulders);
            for pitched in [up, down] {
                assert!((pitched.distance(pivot) - level.distance(pivot)).abs() < 0.01);
            }
        }
        // The shoulders are the pivot: aiming turns the arms about them, it does not move them.
        assert!(at(&up, Bone::Shoulders).distance(at(&level, Bone::Shoulders)) < 0.02);
    }
}
