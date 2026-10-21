//! `Value<'gc>`: a Scheme value branded with the mutation context it came
//! from (`PRD/GC_PRD.md` §11.3, #703).
//!
//! The word is today's [`TaggedValue`]. The brand `'gc` is the lifetime of
//! one [`Cx`](crate::Cx), invariant, so a value can neither outlive the
//! context it came from nor be handed to another. No collection runs while a
//! `Cx` lives, so a value in hand is never freed under it. What must outlive
//! the context goes into a root first: an [`Owned`](crate::Owned) handle, or
//! a resumable primitive's state.
//!
//! - **Immediates** (fixnums, characters, booleans, the empty list,
//!   unspecified, eof) need no context: [`Value::fixnum`], [`Value::NULL`],
//!   [`Value::immediate`].
//! - **A heap word** becomes a `Value` only through [`Value::from_tagged`],
//!   which is `unsafe`: the trusted island's way in, where the machine
//!   vouches for the word.
//! - **Reading the word** is safe ([`Value::tagged`], or `*v` through
//!   `Deref`), so the immediate predicates of `TaggedValue` (`is_fixnum`,
//!   `as_char`, …) work on a value unchanged.
//!
//! A value is neither `Send` nor `Sync`. The heap is one carrier's, and a
//! value in a `static` would be a root no collection sees: the compiler
//! refuses one, as the second example below shows.
//!
//! ```
//! use patina_core::Value;
//!
//! let three = Value::fixnum(3);
//! assert_eq!(three.as_fixnum(), Some(3));
//! assert!(Value::NULL.is_null());
//! ```
//!
//! A constant compiles, since no collection need see it:
//!
//! ```
//! use patina_core::Value;
//!
//! const NIL: Value<'static> = Value::NULL;
//! assert!(NIL.is_null());
//! ```
//!
//! A value in a `static` does not (#709):
//!
//! ```compile_fail,E0277
//! use patina_core::Value;
//!
//! static NIL: Value<'static> = Value::NULL;
//! ```

use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;

use crate::tagged_value::TaggedValue;

/// The brand: invariant in `'gc` (`fn(&'gc ()) -> &'gc ()`), and neither
/// `Send` nor `Sync` (`*const ()`).
type Brand<'gc> = PhantomData<(fn(&'gc ()) -> &'gc (), *const ())>;

/// A Scheme value, valid while the [`Cx`](crate::Cx) it came from lives.
/// See the [module docs](self).
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Value<'gc> {
    word: TaggedValue,
    brand: Brand<'gc>,
}

impl<'gc> Value<'gc> {
    /// `#f`.
    pub const FALSE: Self = Self::of(TaggedValue::FALSE);
    /// `#t`.
    pub const TRUE: Self = Self::of(TaggedValue::TRUE);
    /// The empty list.
    pub const NULL: Self = Self::of(TaggedValue::NULL);
    /// The end-of-file object.
    pub const EOF: Self = Self::of(TaggedValue::EOF);
    /// The unspecified value.
    pub const UNSPECIFIED: Self = Self::of(TaggedValue::UNSPECIFIED);

    /// Brand a word: private, since only immediates may be branded safely.
    const fn of(word: TaggedValue) -> Self {
        Self {
            word,
            brand: PhantomData,
        }
    }

    /// A fixnum. Panics where [`TaggedValue::fixnum`] does, outside the
    /// fixnum range.
    #[inline]
    pub fn fixnum(n: i64) -> Self {
        Self::of(TaggedValue::fixnum(n))
    }

    /// A character.
    #[inline]
    pub fn character(c: char) -> Self {
        Self::of(TaggedValue::character(c))
    }

    /// A boolean.
    #[inline]
    pub fn boolean(b: bool) -> Self {
        Self::of(TaggedValue::boolean(b))
    }

    /// `word` as a value if it is an immediate, which needs no context: no
    /// collection can free it.
    #[inline]
    pub fn immediate(word: TaggedValue) -> Option<Self> {
        word.is_immediate().then_some(Self::of(word))
    }

    /// Brand any word, heap words included.
    ///
    /// # Safety
    ///
    /// `word` must be live in the heap of the `Cx` whose brand `'gc` is, for
    /// as long as `'gc` lasts: reachable from a root, or held where no
    /// collection can run before it is used. The machines vouch for the
    /// words in their registers and steps; nothing else may call this.
    #[inline]
    pub const unsafe fn from_tagged(word: TaggedValue) -> Self {
        Self::of(word)
    }

    /// The word, for today's `TaggedValue` API. Reading it is safe; making a
    /// value of a heap word is not ([`Value::from_tagged`]).
    #[inline]
    pub const fn tagged(self) -> TaggedValue {
        self.word
    }

    /// Words as values, by reference: the trusted island's bulk form of
    /// [`Value::from_tagged`].
    ///
    /// # Safety
    ///
    /// As for [`Value::from_tagged`], for every word.
    #[inline]
    pub unsafe fn from_tagged_slice(words: &[TaggedValue]) -> &[Self] {
        // SAFETY: `Value` is `repr(transparent)` over `TaggedValue`, so the
        // layouts match; the caller vouches for the words.
        unsafe { &*(std::ptr::from_ref(words) as *const [Self]) }
    }

    /// Values as words, by reference.
    #[inline]
    pub fn tagged_slice(values: &[Self]) -> &[TaggedValue] {
        // SAFETY: `Value` is `repr(transparent)` over `TaggedValue`, and
        // reading words is safe.
        unsafe { &*(std::ptr::from_ref(values) as *const [TaggedValue]) }
    }
}

/// The word's immediate predicates (`is_fixnum`, `as_char`, `is_pair`, …)
/// read through to [`TaggedValue`].
impl Deref for Value<'_> {
    type Target = TaggedValue;

    #[inline]
    fn deref(&self) -> &TaggedValue {
        &self.word
    }
}

impl fmt::Debug for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.word, f)
    }
}

impl fmt::Display for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.word, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediates_are_branded_without_a_context() {
        assert_eq!(Value::fixnum(7).as_fixnum(), Some(7));
        assert_eq!(Value::character('λ').as_char(), Some('λ'));
        assert!(Value::boolean(true).is_truthy());
        assert_eq!(Value::immediate(TaggedValue::NULL), Some(Value::NULL));
    }

    #[test]
    fn a_heap_word_is_no_immediate() {
        let mut heap = crate::Heap::new();
        let pair = heap.alloc_pair(TaggedValue::NULL, TaggedValue::NULL);
        assert_eq!(Value::immediate(pair), None);
    }

    #[test]
    fn slices_keep_their_words() {
        let words = [TaggedValue::fixnum(1), TaggedValue::TRUE];
        // SAFETY: immediates only.
        let values = unsafe { Value::from_tagged_slice(&words) };
        assert_eq!(values[0].as_fixnum(), Some(1));
        assert_eq!(Value::tagged_slice(values), &words);
    }
}
