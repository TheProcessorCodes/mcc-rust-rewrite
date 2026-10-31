//! Script values.

use std::sync::Arc;

use blam_script::ValueType;

/// An object as scripts see it.
///
/// - below [`ObjectRef::PLAYER`]: an index into the scenario's object names;
/// - [`ObjectRef::PLAYER`] + n: player n (only player 0 exists here);
/// - from [`ObjectRef::DYNAMIC`] up: objects other systems made (the AI
///   hooks hand these out for actors and their vehicles).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectRef(pub u32);

impl ObjectRef {
    pub const PLAYER: u32 = 0x1_0000;
    pub const DYNAMIC: u32 = 0x100_0000;

    pub fn player(n: u32) -> ObjectRef {
        ObjectRef(Self::PLAYER + n)
    }
    /// The scenario object name index, for named objects.
    pub fn name_index(self) -> Option<u16> {
        (self.0 < Self::PLAYER).then_some(self.0 as u16)
    }
    pub fn player_index(self) -> Option<u32> {
        (Self::PLAYER..Self::DYNAMIC).contains(&self.0).then(|| self.0 - Self::PLAYER)
    }
}

#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Void,
    Bool(bool),
    Real(f32),
    Short(i16),
    Long(i32),
    Text(Arc<str>),
    Object(Option<ObjectRef>),
    Objects(Arc<Vec<ObjectRef>>),
    /// A typed index or id: a trigger volume, cutscene flag, point
    /// reference, AI, tag, enum value, string id... `text` is the name the
    /// source used, when the map kept it.
    Ref {
        ty: ValueType,
        value: u32,
        text: Option<Arc<str>>,
    },
}

impl Value {
    pub fn num(&self) -> f64 {
        match self {
            Value::Void => 0.0,
            Value::Bool(b) => *b as i32 as f64,
            Value::Real(r) => *r as f64,
            Value::Short(s) => *s as f64,
            Value::Long(l) => *l as f64,
            Value::Text(_) => 0.0,
            Value::Object(o) => o.map_or(-1.0, |o| o.0 as f64),
            Value::Objects(l) => l.len() as f64,
            Value::Ref { value, .. } => (*value as u16 as i16) as f64,
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Object(o) => o.is_some(),
            Value::Objects(l) => !l.is_empty(),
            Value::Text(t) => !t.is_empty(),
            Value::Void => false,
            v => v.num() != 0.0,
        }
    }

    pub fn int(&self) -> i32 {
        self.num().round() as i32
    }

    /// The low 16 bits of a typed index, as an index (`None` for -1).
    pub fn index(&self) -> Option<usize> {
        match self {
            Value::Ref { value, .. } => usize::try_from(*value as u16 as i16).ok(),
            Value::Short(s) => usize::try_from(*s).ok(),
            Value::Long(l) => usize::try_from(*l).ok(),
            _ => None,
        }
    }

    pub fn raw(&self) -> u32 {
        match self {
            Value::Ref { value, .. } => *value,
            v => v.int() as u32,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Value::Text(t) => Some(t),
            Value::Ref { text, .. } => text.as_deref(),
            _ => None,
        }
    }

    /// The objects a value names: one object, or a list.
    pub fn objects(&self) -> Vec<ObjectRef> {
        match self {
            Value::Object(Some(o)) => vec![*o],
            Value::Objects(l) => l.as_ref().clone(),
            _ => Vec::new(),
        }
    }

    pub fn object(&self) -> Option<ObjectRef> {
        match self {
            Value::Object(o) => *o,
            Value::Objects(l) => l.first().copied(),
            _ => None,
        }
    }

    /// Equality the way `=` sees it.
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Text(a), Value::Text(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a == b,
            (Value::Objects(a), Value::Objects(b)) => a == b,
            (a, b) => a.num() == b.num(),
        }
    }

    /// The value as a script of type `ty` holds it.
    pub fn coerce(self, ty: ValueType) -> Value {
        match ty {
            ValueType::BOOLEAN => Value::Bool(self.truthy()),
            ValueType::REAL => Value::Real(self.num() as f32),
            ValueType::SHORT => Value::Short(self.num().round().clamp(i16::MIN as f64, i16::MAX as f64) as i16),
            ValueType::LONG => Value::Long(self.num().round() as i32),
            ValueType::OBJECT_LIST => match self {
                Value::Object(o) => Value::Objects(Arc::new(o.into_iter().collect())),
                v => v,
            },
            t if t.names_object() => match self {
                Value::Objects(l) => Value::Object(l.first().copied()),
                v => v,
            },
            _ => self,
        }
    }

    /// What a function of type `ty` returns when nothing better is known.
    pub fn default_of(ty: ValueType) -> Value {
        match ty {
            ValueType::BOOLEAN => Value::Bool(false),
            ValueType::REAL => Value::Real(0.0),
            ValueType::SHORT => Value::Short(0),
            ValueType::LONG => Value::Long(0),
            ValueType::OBJECT_LIST => Value::Objects(Arc::new(Vec::new())),
            t if t.names_object() => Value::Object(None),
            ValueType::VOID => Value::Void,
            t => Value::Ref {
                ty: t,
                value: u32::MAX,
                text: None,
            },
        }
    }
}
