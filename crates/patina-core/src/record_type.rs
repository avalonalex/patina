use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Global counter for generating unique record type IDs (generative semantics)
static RECORD_TYPE_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a new unique record type ID
///
/// Each call to define-record-type creates a new type with a unique ID.
/// This ensures generative semantics: two record types with the same
/// name and fields are still distinct types.
pub fn next_record_type_id() -> usize {
    RECORD_TYPE_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// Record type descriptor - represents a record type itself
///
/// Created by `define-record-type`. Each invocation creates a new descriptor
/// with a unique ID (generative semantics), even if the name and fields match
/// a previous definition.
///
/// # R7RS Compliance
///
/// From R7RS Section 5.5:
/// > The define-record-type construct is generative: each use creates a new
/// > record type that is distinct from all existing types, including Scheme's
/// > predefined types and other record types — even record types of the same
/// > name or structure.
#[derive(Debug, Clone)]
pub struct RecordTypeDescriptor {
    /// Unique identifier for this record type (generative semantics)
    pub id: usize,
    /// Name of the record type (for display purposes)
    pub name: Rc<str>,
    /// Field names in declaration order
    pub fields: Vec<Rc<str>>,
}

impl RecordTypeDescriptor {
    /// Create a new record type descriptor with a unique ID
    pub fn new(name: &str, fields: Vec<String>) -> Self {
        RecordTypeDescriptor {
            id: next_record_type_id(),
            name: Rc::from(name),
            fields: fields.into_iter().map(|s| Rc::from(s.as_str())).collect(),
        }
    }

    /// Get the index of a field by name, if it exists
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.as_ref() == name)
    }
}

impl PartialEq for RecordTypeDescriptor {
    fn eq(&self, other: &Self) -> bool {
        // Identity based on unique ID only (generative semantics)
        // Two record types with the same name/fields are still distinct
        self.id == other.id
    }
}

impl Eq for RecordTypeDescriptor {}
