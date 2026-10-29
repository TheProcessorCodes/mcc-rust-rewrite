//! Where other systems plug into the scripts: the AI (squads, actors,
//! command scripts) and object animation (device machines). Both have a
//! do-nothing default so the scripts run before either exists.
//!
//! The enemies agent implements [`AiHooks`] and installs it with
//! `app.insert_resource(ScriptAi::new(MyAi { .. }))`; the animation agent
//! implements [`DevicePoser`] and installs `ScriptAnimation::new(..)`. See
//! `docs/format/halo-script.md` 10.

use glam::Mat4;

use crate::value::{ObjectRef, Value};

/// What a hook may look at while it runs: the player and the scenario's
/// named places, all in Halo world units.
pub struct HookContext<'a> {
    pub player: Option<glam::Vec3>,
    /// The scenario's AI names are not decoded here; a squad or starting
    /// location constant arrives as `Value::Ref { ty: ai, value, text }`
    /// with the name the source used in `text` (`warthog01/driver`).
    pub data: &'a blam_script::ScriptData,
    /// Seconds since the scripts started.
    pub time: f64,
}

/// The AI side of HaloScript: every `ai_*`, `cs_*` and `vs_*` call, and the
/// unit and vehicle queries that need actors (`ai_vehicle_get_from_starting_location`,
/// `ai_get_object`, `object_get_ai`, `vehicle_driver`, ...).
///
/// `call` gets the function's name and its evaluated arguments and returns
/// its value, or `None` when it does not handle that function: the script
/// then gets the type's default (`0`, `false`, no object), which lets
/// `(sleep_until (< (ai_living_count x) 1))` pass and the mission go on.
/// Objects the AI makes (actors, their vehicles) should be handed out as
/// `ObjectRef(ObjectRef::DYNAMIC + n)`.
pub trait AiHooks: Send + Sync + 'static {
    fn call(&mut self, function: &str, args: &[Value], ctx: &HookContext<'_>) -> Option<Value>;

    /// Where an AI-made object is, Halo world units (for
    /// `volume_test_object` and distances).
    fn object_position(&self, _object: ObjectRef) -> Option<glam::Vec3> {
        None
    }

    /// Called every script tick (30 a second) after the scripts ran.
    fn tick(&mut self, _ctx: &HookContext<'_>) {}
}

/// No AI: every call falls through to the default value.
pub struct NoAi;

impl AiHooks for NoAi {
    fn call(&mut self, _function: &str, _args: &[Value], _ctx: &HookContext<'_>) -> Option<Value> {
        None
    }
}

/// Poses a device machine (a door, a lift, the Warthog run's falling
/// islands) from its animation, given the position and overlay tracks
/// scripts chose and how far along each is (0..1). Returns the object's
/// local transform relative to its placement, or `None` to keep the
/// built-in stand-in (`objects::fallback_device_pose`).
///
/// `object` is the `Scene::objects` index, `tag` its object tag (`mach`,
/// whose `hlmt` names the `jmad`). Meant to sit on the animation crate:
/// load the machine's animations from `tag` once, drive its "position"
/// animation with `position` (a falling `track` such as the Warthog run
/// islands' `other_fall` may map to a named animation like `fall`), and
/// return the root's transform.
pub trait DevicePoser: Send + Sync + 'static {
    fn pose(
        &self,
        object: u32,
        tag: u32,
        track: Option<&str>,
        position: f32,
        overlay_track: Option<&str>,
        overlay: f32,
    ) -> Option<Mat4>;
}
