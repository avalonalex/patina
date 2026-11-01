//! Where to look among one spelling's scoped bindings (#613).
//!
//! A reference to a macro-introduced definition is resolved against every
//! binding of its spelling (`scope_resolve::resolve_index`), and a spelling a
//! macro introduces again at each expansion has one binding per expansion. A
//! scan of them all made `n` expansions cost time quadratic in `n` on both
//! backends, and on the tree-walker every read of such a variable cost time
//! proportional to the expansions before it.
//!
//! [`ScopeIndex`] files each binding's position under one scope of its set,
//! and answers a reference with the positions filed under the scopes it
//! carries. It is built only for a list longer than [`INDEX_ABOVE`] in a
//! namespace, or [`FRAME_INDEX_ABOVE`] in a frame; below that, a scan of the
//! list is the cheaper answer.

use crate::scope::{ScopeId, ScopeSet};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

/// A namespace's list of more than this many scoped bindings of one spelling
/// is indexed.
///
/// Below it a scan visits fewer bindings than the index would probe scopes:
/// a reference carries a few scopes, each a hash probe, where a short list is
/// a few subset tests over inline sets.
pub(crate) const INDEX_ABOVE: usize = 8;

/// A frame's list of more than this many scoped bindings of one spelling is
/// indexed.
///
/// Higher than a namespace's because a frame the tree-walker builds per call
/// holds what one form binds: its lists are bounded by the code, not by how
/// often the code runs, so a scan of one does not grow with the program's
/// history, which is what #613 was. And some forms bind many of one
/// spelling. A `let-values` clause of nine formals binds nine parameters
/// spelled `x`, each at its own expansion's scope, so at [`INDEX_ABOVE`]
/// every call of it would build an index, and its environment's out-of-line
/// tables, on the path that binds parameters. A frame that lives long and
/// grows, as a host's environment does under a stream of expansions (#620),
/// is still indexed once it passes this.
pub(crate) const FRAME_INDEX_ABOVE: usize = 64;

/// No position: the end of a chain.
const NONE: u32 = u32::MAX;

/// The positions of a list of bindings, each filed under its set's *key*:
/// its largest scope.
///
/// **Why one scope is enough.** A binding is a candidate for a reference
/// when its scope set is a subset of the reference's
/// (`scope_resolve::is_candidate`), so every scope of a candidate's set, its
/// key among them, is one the reference carries. The buckets of the
/// reference's own scopes therefore hold every candidate, and a binding filed
/// anywhere else is not one. What the buckets hold is a superset: a binding
/// whose key the reference carries may still lack another of its scopes, so
/// the caller tests each position with `is_candidate` as before. The index
/// only removes bindings that test would refuse.
///
/// **Why the largest.** Scopes are numbered as they are made, so a set's
/// largest is the one minted last, which for a macro-introduced definition is
/// typically the scope of the expansion that introduced it: the scope its
/// rivals of the same spelling do not share. `n` expansions of one macro
/// file `n` bindings of one spelling under `n` keys, one each, and a
/// reference from inside one expansion probes one bucket that holds one
/// binding. Any scope of the set would be correct; this one is selective.
///
/// **Positions, not bindings.** The list a position points into stays the
/// one copy of the bindings, and this only says where in it to look. Two
/// copies of one table are how a lookup came to miss a binding that scoped
/// resolution still found (Larceny triage families 36 and 38).
///
/// Each key's positions form a chain, newest first, through `previous`, so
/// the common bucket of one position costs one map entry and one `u32`.
/// Nothing is ever removed: the lists this indexes only grow.
#[derive(Debug)]
pub(crate) struct ScopeIndex {
    /// Key scope → the newest position filed under it.
    heads: FxHashMap<ScopeId, u32>,
    /// The newest position whose set is empty, which is a candidate for every
    /// reference and has no key. The lists indexed today hold none — a scoped
    /// binding at the empty set is stored by name instead — so this stays
    /// `NONE`; it is here so that the index is correct for any list.
    unkeyed: u32,
    /// Per position, the position filed before it under the same key, or
    /// `NONE`. Its length is the number of positions filed.
    previous: Vec<u32>,
}

impl Default for ScopeIndex {
    fn default() -> Self {
        ScopeIndex {
            heads: FxHashMap::default(),
            unkeyed: NONE,
            previous: Vec::new(),
        }
    }
}

impl ScopeIndex {
    /// An index of `sets`, the scope sets of a list's bindings in list order.
    pub(crate) fn of<'a>(sets: impl ExactSizeIterator<Item = &'a ScopeSet>) -> Self {
        let mut index = ScopeIndex {
            previous: Vec::with_capacity(sets.len()),
            ..ScopeIndex::default()
        };
        for scopes in sets {
            index.push(scopes);
        }
        index
    }

    /// File the next position of the list, a binding at `scopes`.
    pub(crate) fn push(&mut self, scopes: &ScopeSet) {
        let position = u32::try_from(self.previous.len())
            .expect("a spelling's scoped bindings fit in u32 positions");
        let head = match scopes.largest() {
            Some(key) => self.heads.entry(*key).or_insert(NONE),
            None => &mut self.unkeyed,
        };
        self.previous.push(*head);
        *head = position;
    }

    /// How many positions are filed: the length of the list indexed.
    pub(crate) fn len(&self) -> usize {
        self.previous.len()
    }

    /// The positions whose binding can be a candidate for `reference`, newest
    /// first — the order `scope_resolve::resolve_index` wants within one
    /// environment. Every candidate's position is among them; so may be some
    /// that the caller's `is_candidate` test refuses.
    pub(crate) fn candidates(&self, reference: &ScopeSet) -> SmallVec<[u32; 8]> {
        let mut out: SmallVec<[u32; 8]> = SmallVec::new();
        self.extend_chain(self.unkeyed, &mut out);
        // Probe from the smaller side: a reference's scopes, or the keys when
        // there are fewer of them.
        if self.heads.len() < reference.len() {
            for (key, &head) in &self.heads {
                if reference.contains(key) {
                    self.extend_chain(head, &mut out);
                }
            }
        } else {
            for scope in reference.iter() {
                if let Some(&head) = self.heads.get(scope) {
                    self.extend_chain(head, &mut out);
                }
            }
        }
        // Each chain is newest first, and a position is in exactly one chain,
        // so merging the chains is a sort with nothing to deduplicate. One
        // chain is the usual case, and already sorted.
        out.sort_unstable_by(|a, b| b.cmp(a));
        out
    }

    /// The positions whose binding can be at exactly `scopes`: those filed
    /// under its key, newest first. The caller compares the sets.
    pub(crate) fn exact(&self, scopes: &ScopeSet) -> impl Iterator<Item = usize> + '_ {
        let head = match scopes.largest() {
            Some(key) => self.heads.get(key).copied().unwrap_or(NONE),
            None => self.unkeyed,
        };
        self.chain(head).map(|position| position as usize)
    }

    /// The positions filed under one key, from `head`, newest first.
    fn chain(&self, head: u32) -> impl Iterator<Item = u32> + '_ {
        std::iter::successors((head != NONE).then_some(head), |&position| {
            let previous = self.previous[position as usize];
            (previous != NONE).then_some(previous)
        })
    }

    fn extend_chain(&self, head: u32, out: &mut SmallVec<[u32; 8]>) {
        out.extend(self.chain(head));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope_resolve::is_candidate;

    fn set(scopes: &[usize]) -> ScopeSet {
        scopes.iter().map(|&s| ScopeId(s)).collect()
    }

    /// What a scan answers: every candidate's position, newest first.
    fn scanned(sets: &[ScopeSet], reference: &ScopeSet) -> Vec<usize> {
        (0..sets.len())
            .rev()
            .filter(|&i| is_candidate(&sets[i], reference))
            .collect()
    }

    fn indexed(sets: &[ScopeSet], reference: &ScopeSet) -> Vec<usize> {
        let index = ScopeIndex::of(sets.iter());
        index
            .candidates(reference)
            .into_iter()
            .map(|position| position as usize)
            .filter(|&i| is_candidate(&sets[i], reference))
            .collect()
    }

    /// A small linear congruential generator: the property below wants many
    /// shapes, and a fixed seed keeps a failure reproducible.
    struct Lcg(u64);

    impl Lcg {
        fn below(&mut self, n: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) % n
        }

        fn set(&mut self, universe: u64, most: u64) -> ScopeSet {
            (0..self.below(most + 1))
                .map(|_| ScopeId(self.below(universe) as usize))
                .collect()
        }
    }

    /// The index answers a reference with the candidates a scan finds, in the
    /// scan's order, over lists with shared scopes, repeated keys, equal sets
    /// and empty sets, and references from empty to larger than any binding.
    #[test]
    fn the_index_answers_as_a_scan_does() {
        let mut rng = Lcg(0x613);
        for _ in 0..2_000 {
            let universe = 1 + rng.below(12);
            let sets: Vec<ScopeSet> = (0..rng.below(40)).map(|_| rng.set(universe, 4)).collect();
            for _ in 0..8 {
                let reference = rng.set(universe, 6);
                assert_eq!(
                    indexed(&sets, &reference),
                    scanned(&sets, &reference),
                    "bindings {sets:?}, reference {reference}"
                );
            }
        }
    }

    /// The case #613 is about: one spelling introduced by `n` expansions,
    /// each under a scope of its own. A reference from inside one expansion
    /// is shown that expansion's binding alone.
    #[test]
    fn a_reference_probes_only_its_own_expansions_bucket() {
        let shared = 1;
        let sets: Vec<ScopeSet> = (0..20_000).map(|i| set(&[shared, 100 + i])).collect();
        let index = ScopeIndex::of(sets.iter());
        let reference = set(&[shared, 100 + 4_321, 50_000]);
        assert_eq!(index.candidates(&reference).as_slice(), &[4_321]);
        assert!(index.candidates(&set(&[shared])).is_empty());
    }

    /// `exact` finds the positions that can hold a set, newest first, and a
    /// set filed twice is found at both.
    #[test]
    fn exact_finds_every_position_of_a_key() {
        let sets = [
            set(&[1, 5]),
            set(&[2, 5]),
            set(&[1, 6]),
            set(&[1, 5]),
            set(&[]),
        ];
        let index = ScopeIndex::of(sets.iter());
        assert_eq!(index.exact(&set(&[1, 5])).collect::<Vec<_>>(), [3, 1, 0]);
        assert_eq!(index.exact(&set(&[3, 6])).collect::<Vec<_>>(), [2]);
        assert_eq!(index.exact(&set(&[7])).count(), 0);
        assert_eq!(index.exact(&set(&[])).collect::<Vec<_>>(), [4]);
        assert_eq!(index.len(), sets.len());
    }
}
