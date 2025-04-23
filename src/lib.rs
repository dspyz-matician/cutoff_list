use std::{cmp::Ordering, mem, ptr};

use index_list::IndexList;

// Re-export IndexList's Index type for convenience.
pub use index_list::Index;

/// A list structure, backed by a `Vec` managed by `index_list::IndexList`,
/// where each element tracks how many predefined "cutoff points" precede or
/// coincide with its position in the list sequence.
///
/// `index_list::IndexList` provides a doubly-linked list interface but uses a `Vec`
/// for storage. An `Index` is a stable identifier (conceptually like a slot index)
/// that maps to the current location of an element within the internal `Vec`,
/// remaining valid even if other elements are added or removed.
///
/// This `CutoffList` structure builds upon `IndexList` to efficiently manage
/// lists that are conceptually divided into segments based on position (e.g., for
/// segmented LRU caches). It maintains internal caches related to the cutoffs.
pub struct CutoffList<T> {
    /// The sorted list of cutoff positions. Each value `p` represents a conceptual
    /// cutoff occurring *before* list position `p`. E.g., `[5, 10]` means
    /// cutoffs occur before position 5 and before position 10. Positions are 0-based.
    /// Must be sorted non-decreasingly.
    cutoff_positions: Vec<usize>,
    /// A cache where `following_ind[q]` stores the `Index` of the *first* list
    /// element whose position `p` satisfies `p >= cutoff_positions[q]`.
    /// If no such element exists (e.g., cutoff is beyond the list length),
    /// the index is `None` (`Index::default()`).
    following_ind: Vec<Index>,
    /// The underlying `IndexList` storing entries using `Vec`-based allocation.
    list: IndexList<Entry<T>>,
}

/// Internal entry storing the user's value and cutoff bookkeeping.
struct Entry<T> {
    /// The value stored in the list node.
    value: T,
    /// The number of cutoff positions that occur *before or at* this entry's
    /// current position in the list sequence.
    ///
    /// Specifically, if the element is at position `p` (0-based index in the
    /// sequence), this value is the count of `c` in `cutoff_positions`
    /// such that `c <= p`.
    preceding_cutoffs: usize,
}

impl<T> CutoffList<T> {
    /// Creates a new `CutoffList` with the specified cutoff positions.
    ///
    /// Cutoff positions define conceptual boundaries within the list. For example,
    /// `cutoff_positions = vec![10, 50]` defines two cutoffs, one occurring before
    /// list position 10, and another before list position 50.
    ///
    /// The list is initially empty.
    ///
    /// # Panics
    ///
    /// Panics if `cutoff_positions` is not sorted in non-decreasing order.
    pub fn new(cutoff_positions: Vec<usize>) -> Self {
        assert!(
            cutoff_positions.is_sorted(),
            "{:?} is not sorted",
            cutoff_positions
        );
        Self {
            following_ind: vec![Index::new(); cutoff_positions.len()],
            cutoff_positions,
            list: IndexList::new(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Inserts a value at the front (position 0) of the list.
    ///
    /// Updates internal cutoff counts for shifted elements and the `following_ind` cache.
    ///
    /// # Returns
    ///
    /// The `Index` of the newly inserted element.
    pub fn insert_first(&mut self, value: T) -> Index {
        let new_last_pos = self.list.len();
        let preceding_cutoffs = self.count_leading_zeros();
        let new_index = self.list.insert_first(Entry {
            value,
            preceding_cutoffs,
        });
        let mut iter = self
            .cutoff_positions
            .iter()
            .zip(self.following_ind.iter_mut());
        for _ in 0..preceding_cutoffs {
            let (_, ind) = iter.next().unwrap();
            *ind = new_index;
        }
        for (p, ind) in iter {
            if ind.is_some() {
                assert!(*p < new_last_pos);
                *ind = self.list.prev_index(*ind);
            } else if *p == new_last_pos {
                *ind = self.list.last_index();
            } else {
                assert!(*p > new_last_pos);
                break;
            }
            let preceding_cutoffs = &mut self.list.get_mut(*ind).unwrap().preceding_cutoffs;
            assert!(ptr::eq(p, &self.cutoff_positions[*preceding_cutoffs]));
            *preceding_cutoffs += 1;
        }
        new_index
    }

    /// Inserts a value at the back (end) of the list.
    ///
    /// Updates internal cutoff counts and caches if the new element falls exactly
    /// on a cutoff position that was previously beyond the end of the list.
    ///
    /// # Returns
    ///
    /// The `Index` of the newly inserted element.
    pub fn insert_last(&mut self, value: T) -> Index {
        let mut preceding_cutoffs = 0;
        let mut at_q = None;
        let new_pos = self.list.len();
        for (q, &cutoff) in self.cutoff_positions.iter().enumerate() {
            match cutoff.cmp(&new_pos) {
                Ordering::Less => {}
                Ordering::Equal => {
                    at_q.get_or_insert(q);
                }
                Ordering::Greater => break,
            }
            preceding_cutoffs += 1;
        }
        let new_index = self.list.insert_last(Entry {
            value,
            preceding_cutoffs,
        });
        if let Some(q) = at_q {
            for (&cutoff, ind) in self.cutoff_positions[q..]
                .iter()
                .zip(self.following_ind[q..].iter_mut())
            {
                assert!(ind.is_none());
                if cutoff == new_pos {
                    *ind = new_index;
                } else {
                    assert!(cutoff > new_pos);
                    break;
                }
            }
        }
        new_index
    }

    /// Moves the element specified by `ind` to the front (position 0) of the list.
    ///
    /// Updates the `preceding_cutoffs` count for the moved element and all elements
    /// shifted back, and updates the `following_ind` cache.
    /// If `ind` is already the first element or is invalid, this is a no-op.
    pub fn shift_to_front(&mut self, ind: Index) {
        let prev_ind = self.list.prev_index(ind);
        if prev_ind.is_none() {
            // Either we're already at the front or this index isn't in the list. In either case no change occurs.
            return;
        }
        let leading_zeros = self.count_leading_zeros();
        let preceding_cutoffs = mem::replace(
            &mut self.list.get_mut(ind).unwrap().preceding_cutoffs,
            leading_zeros,
        );
        let shifted = self.list.shift_index_to_front(ind);
        assert!(shifted);
        let mut iter = self.following_ind.iter_mut();
        for _ in 0..leading_zeros {
            *iter.next().unwrap() = ind;
        }
        for _ in leading_zeros..preceding_cutoffs {
            let cutoff_ind = iter.next().unwrap();
            if *cutoff_ind == ind {
                *cutoff_ind = prev_ind;
            } else {
                assert!(cutoff_ind.is_some());
                *cutoff_ind = self.list.prev_index(*cutoff_ind);
            }
            let preceding_cutoffs = &mut self.list.get_mut(*cutoff_ind).unwrap().preceding_cutoffs;
            *preceding_cutoffs += 1;
        }
    }

    /// Removes the element specified by `index` from the list.
    ///
    /// Updates `preceding_cutoffs` counts for elements that were previously after
    /// the removed element, and updates the `following_ind` cache.
    ///
    /// # Returns
    ///
    /// The value of the removed element, or `None` if `index` was invalid.
    pub fn remove(&mut self, index: Index) -> Option<T> {
        let next_index = self.list.next_index(index);
        let removed = self.list.remove(index)?;
        let preceding_cutoffs = removed.preceding_cutoffs;
        let (before, after) = self.following_ind.split_at_mut(preceding_cutoffs);
        for ind in before.iter_mut().rev() {
            if *ind == index {
                *ind = next_index;
            } else {
                break;
            }
        }
        for ind in after.iter_mut() {
            if ind.is_none() {
                break;
            }
            self.list.get_mut(*ind).unwrap().preceding_cutoffs -= 1;
            *ind = self.list.next_index(*ind);
        }
        Some(removed.value)
    }

    /// Counts how many entries in `cutoff_positions` are 0.
    #[inline]
    fn count_leading_zeros(&mut self) -> usize {
        self.cutoff_positions
            .iter()
            .take_while(|&&cutoff| cutoff == 0)
            .count()
    }

    /// Returns the number of cutoffs that precede or coincide with the element at `ind`.
    ///
    /// Returns `None` if `ind` is not a valid index in the list.
    #[inline]
    pub fn preceding_cutoffs(&self, ind: Index) -> Option<usize> {
        self.list.get(ind).map(|entry| entry.preceding_cutoffs)
    }

    /// Returns the cached `Index` of the first list element whose position `p`
    /// is greater than or equal to `self.cutoff_positions[q]`.
    ///
    /// If `q` is out of bounds or if the `q`-th cutoff position is beyond the
    /// list's end, returns `Index::default()` (representing `None`).
    #[inline]
    pub fn following_ind(&self, q: usize) -> Index {
        self.following_ind.get(q).copied().unwrap_or_default()
    }

    /// Returns an immutable reference to the value stored at the given `ind`.
    ///
    /// Returns `None` if `ind` is not a valid index in the list.
    #[inline]
    pub fn get(&self, ind: Index) -> Option<&T> {
        self.list.get(ind).map(|entry| &entry.value)
    }

    /// Returns the index of the first element (position 0) in the list.
    /// Returns `Index::default()` (representing `None`) if the list is empty.
    #[inline]
    pub fn first_index(&self) -> Index {
        self.list.first_index()
    }

    /// Returns the index of the element immediately following the one specified by `ind`.
    /// Returns `Index::default()` (representing `None`) if `ind` is the last element or invalid.
    #[inline]
    pub fn next_index(&self, ind: Index) -> Index {
        self.list.next_index(ind)
    }

    /// Internal validation function used for testing purposes.
    /// Checks internal consistency between the list, cutoff positions,
    /// preceding cutoff counts, and the following index cache.
    #[cfg(test)]
    fn validate(&self) {
        assert_eq!(self.following_ind.len(), self.cutoff_positions.len());
        assert!(self.cutoff_positions.is_sorted());

        let mut idx = self.list.first_index();
        let mut pos_pass1 = 0; // Current element's position
        let mut calculated_list_len = 0;
        while idx.is_some() {
            let entry = self.list.get(idx).unwrap();

            let expected_prec_cutoffs = self.cutoff_positions.partition_point(|&c| c <= pos_pass1);
            assert_eq!(entry.preceding_cutoffs, expected_prec_cutoffs);

            idx = self.list.next_index(idx);
            pos_pass1 += 1;
            calculated_list_len = pos_pass1;
        }
        assert_eq!(calculated_list_len, self.list.len());

        let mut list_node_idx = self.list.first_index();
        let mut list_node_pos = 0;

        for q in 0..self.cutoff_positions.len() {
            let cutoff_pos = self.cutoff_positions[q];
            let stored_following_idx = self.following_ind[q];

            while list_node_idx.is_some() && list_node_pos < cutoff_pos {
                list_node_idx = self.list.next_index(list_node_idx);
                list_node_pos += 1;
            }

            let actual_following_idx = list_node_idx;

            assert_eq!(stored_following_idx, actual_following_idx);
        }
    }
}

#[cfg(test)]
mod tests;
