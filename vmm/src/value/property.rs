use super::Value;

// Access control for object properties.
// These map roughly to Java visibility modifiers.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyAccess {
    // Read and write from anywhere
    Public,
    // Read from anywhere, no writes ever
    Readonly,
    // Read + write only through `this.prop` (like Java protected)
    Protected,
    // Read only through `this.prop`, no writes ever (like Java protected final)
    ProtectedReadonly,
}

impl Default for PropertyAccess {
    fn default() -> Self {
        PropertyAccess::Public
    }
}

impl PropertyAccess {
    pub fn to_byte(&self) -> u8 {
        match self {
            PropertyAccess::Public => 0,
            PropertyAccess::Readonly => 1,
            PropertyAccess::Protected => 2,
            PropertyAccess::ProtectedReadonly => 3,
        }
    }

    pub fn from_byte(b: u8) -> Result<Self, String> {
        match b {
            0 => Ok(PropertyAccess::Public),
            1 => Ok(PropertyAccess::Readonly),
            2 => Ok(PropertyAccess::Protected),
            3 => Ok(PropertyAccess::ProtectedReadonly),
            _ => Err(format!("unknown property access byte: {}", b)),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct PropertyDescriptor {
    pub value: Value,
    pub access: PropertyAccess,
}

impl PropertyDescriptor {
    pub fn public(value: Value) -> Self {
        PropertyDescriptor {
            value,
            access: PropertyAccess::Public,
        }
    }

    pub fn with_access(value: Value, access: PropertyAccess) -> Self {
        PropertyDescriptor { value, access }
    }

    // Can external code (not this.prop) read this?
    pub fn readable_externally(&self) -> bool {
        matches!(
            self.access,
            PropertyAccess::Public | PropertyAccess::Readonly
        )
    }

    // Can external code write this?
    pub fn writable_externally(&self) -> bool {
        matches!(self.access, PropertyAccess::Public)
    }

    // Can this.prop read this? (always yes)
    pub fn readable_by_this(&self) -> bool {
        true
    }

    // Can this.prop write this?
    pub fn writable_by_this(&self) -> bool {
        matches!(
            self.access,
            PropertyAccess::Public | PropertyAccess::Protected
        )
    }
}
