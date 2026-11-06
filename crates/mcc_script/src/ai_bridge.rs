//! The scripts' AI calls handed to the enemies (`mcc_ai`) through
//! `mcc_core::ai::AiBridge`: `ai_place`, `ai_erase`, `ai_kill`,
//! `ai_allegiance` / `ai_allegiance_remove`, `ai_living_count`,
//! `ai_spawn_count`. Everything else returns `None` (the type's default).
//!
//! Commands are queued and go out the next frame; the census comes back
//! every frame.

use std::collections::HashSet;

use mcc_core::ai::{AiBridge, AiCommand, AiRef, team};

use crate::hooks::{AiHooks, HookContext};
use crate::value::Value;

/// [`AiHooks`] over the AI bridge. Install with
/// `ScriptAi::new(BridgeAi::new(bridge.clone()))`.
pub struct BridgeAi {
    bridge: AiBridge,
    /// Names already logged as unknown, so each is said once.
    logged: HashSet<String>,
}

impl BridgeAi {
    pub fn new(bridge: AiBridge) -> BridgeAi {
        BridgeAi {
            bridge,
            logged: HashSet::new(),
        }
    }

    fn log_once(&mut self, key: String, message: impl FnOnce() -> String) {
        if self.logged.insert(key) {
            eprintln!("scripts: {}", message());
        }
    }
}

/// Who an AI argument names: `all`, or a squad / group / `squad/location`
/// by the name the script used.
fn who(v: &Value) -> Option<AiRef> {
    let name = v.text()?;
    Some(if name == "all" { AiRef::All } else { AiRef::named(name) })
}

/// A team argument (`ai_allegiance player human`): by name, else by the
/// enum's index, which is the unit tags' team number.
fn team_of(v: &Value) -> Option<i16> {
    let by_name = v.text().and_then(|t| {
        Some(match t {
            "default" => team::DEFAULT,
            "player" => team::PLAYER,
            "human" => team::HUMAN,
            "covenant" => team::COVENANT,
            "flood" => team::FLOOD,
            "sentinel" => team::SENTINEL,
            "heretic" => team::HERETIC,
            "prophet" => team::PROPHET,
            "guilty" => team::GUILTY,
            _ => return None,
        })
    });
    by_name.or_else(|| v.index().and_then(|i| i16::try_from(i).ok()))
}

impl AiHooks for BridgeAi {
    fn call(&mut self, function: &str, args: &[Value], _ctx: &HookContext<'_>) -> Option<Value> {
        let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
        match function {
            "ai_place" => {
                let Some(who) = who(&arg(0)) else {
                    self.log_once(format!("ai_place {:?}", arg(0).raw()), || format!("ai_place of an unnamed AI ({:#x}): nobody placed", arg(0).raw()));
                    return Some(Value::Void);
                };
                let count = args.get(1).map(|c| c.int().max(0) as u32);
                self.bridge.send(AiCommand::Place { who, count });
                Some(Value::Void)
            }
            "ai_erase" => {
                if let Some(who) = who(&arg(0)) {
                    self.bridge.send(AiCommand::Erase { who });
                }
                Some(Value::Void)
            }
            "ai_kill" | "ai_kill_silent" => {
                if let Some(who) = who(&arg(0)) {
                    self.bridge.send(AiCommand::Kill { who });
                }
                Some(Value::Void)
            }
            "ai_allegiance" | "ai_allegiance_remove" => {
                if let (Some(a), Some(b)) = (team_of(&arg(0)), team_of(&arg(1))) {
                    self.bridge.send(AiCommand::Allegiance {
                        a,
                        b,
                        allied: function == "ai_allegiance",
                    });
                }
                Some(Value::Void)
            }
            "ai_living_count" => {
                let n = match who(&arg(0)) {
                    Some(AiRef::Named(name)) => self.bridge.living_count(Some(&name)),
                    _ => self.bridge.living_count(None),
                };
                Some(Value::Short(n.min(i16::MAX as u32) as i16))
            }
            "ai_spawn_count" => {
                let n = match who(&arg(0)) {
                    Some(AiRef::Named(name)) => self.bridge.spawn_count(&name),
                    _ => 0,
                };
                Some(Value::Short(n.min(i16::MAX as u32) as i16))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn ai(name: &str) -> Value {
        Value::Ref {
            ty: blam_script::ValueType::AI,
            value: 0x8000_0000,
            text: Some(Arc::from(name)),
        }
    }

    fn ctx(data: &blam_script::ScriptData) -> HookContext<'_> {
        HookContext {
            player: None,
            data,
            time: 0.0,
        }
    }

    #[test]
    fn commands_and_counts_go_through_the_bridge() {
        let bridge = AiBridge::default();
        let mut hooks = BridgeAi::new(bridge.clone());
        let data = blam_script::ScriptData::default();
        let c = ctx(&data);
        hooks.call("ai_place", &[ai("arbiter_trench/trench")], &c);
        hooks.call("ai_place", &[ai("sentinels"), Value::Short(2)], &c);
        hooks.call("ai_erase", &[ai("all")], &c);
        let team = |t: &str| Value::Ref {
            ty: blam_script::ValueType::AI,
            value: 0,
            text: Some(Arc::from(t)),
        };
        hooks.call("ai_allegiance_remove", &[team("sentinel"), team("player")], &c);
        let queued = std::mem::take(&mut bridge.0.lock().unwrap().queue);
        assert_eq!(queued.len(), 4);
        assert!(matches!(&queued[0], AiCommand::Place { who: AiRef::Named(n), count: None } if n == "arbiter_trench/trench"));
        assert!(matches!(&queued[1], AiCommand::Place { count: Some(2), .. }));
        assert!(matches!(&queued[2], AiCommand::Erase { who: AiRef::All }));
        assert!(matches!(
            queued[3],
            AiCommand::Allegiance {
                a: team::SENTINEL,
                b: team::PLAYER,
                allied: false
            }
        ));

        bridge.0.lock().unwrap().living.insert("trench_100".into(), 3);
        bridge.0.lock().unwrap().total_living = 5;
        assert_eq!(hooks.call("ai_living_count", &[ai("trench_100")], &c).unwrap().int(), 3);
        assert_eq!(hooks.call("ai_living_count", &[ai("all")], &c).unwrap().int(), 5);
        assert!(hooks.call("ai_get_object", &[ai("arbiter_trench/trench")], &c).is_none());
    }
}
