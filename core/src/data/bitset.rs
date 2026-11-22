//! A set of small non-negative integers, which holds the positions of an
//! object's ancestors.

/// A set of small non-negative integers as a bitmap.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BitSet(Vec<u64>);

impl BitSet {
    pub fn contains(&self, bit: usize) -> bool {
        self.0
            .get(bit / 64)
            .is_some_and(|word| word >> (bit % 64) & 1 == 1)
    }

    pub fn insert(&mut self, bit: usize) {
        let word = bit / 64;
        if self.0.len() <= word {
            self.0.resize(word + 1, 0);
        }
        self.0[word] |= 1 << (bit % 64);
    }

    /// Inserts a clear bit at `at`: every bit at or above it moves up one.
    pub fn open(&mut self, at: usize) {
        let bits: Vec<usize> = self.iter().collect();
        *self = BitSet::default();
        for bit in bits {
            self.insert(if bit >= at { bit + 1 } else { bit });
        }
    }

    /// How many positions this set and `mask` both hold.
    pub fn count_within(&self, mask: &BitSet) -> usize {
        self.0
            .iter()
            .zip(&mask.0)
            .map(|(mine, theirs)| (mine & theirs).count_ones() as usize)
            .sum()
    }

    pub fn union_with(&mut self, other: &BitSet) {
        if self.0.len() < other.0.len() {
            self.0.resize(other.0.len(), 0);
        }
        for (mine, theirs) in self.0.iter_mut().zip(&other.0) {
            *mine |= theirs;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().enumerate().flat_map(|(index, word)| {
            (0..64).filter_map(move |bit| (word >> bit & 1 == 1).then_some(index * 64 + bit))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::BitSet;

    #[test]
    fn a_bitset_is_a_set_of_positions() {
        let mut bits = BitSet::default();
        assert!(!bits.contains(0));
        bits.insert(0);
        bits.insert(130);
        assert!(bits.contains(0) && bits.contains(130));
        assert!(!bits.contains(1) && !bits.contains(129) && !bits.contains(999));
        assert_eq!(bits.iter().collect::<Vec<_>>(), vec![0, 130]);

        let mut other = BitSet::default();
        other.insert(64);
        other.union_with(&bits);
        assert_eq!(other.iter().collect::<Vec<_>>(), vec![0, 64, 130]);
        let mut shorter = BitSet::default();
        shorter.insert(1);
        other.union_with(&shorter);
        assert_eq!(other.iter().collect::<Vec<_>>(), vec![0, 1, 64, 130]);
        other.open(64);
        assert_eq!(other.iter().collect::<Vec<_>>(), vec![0, 1, 65, 131]);
    }
}
