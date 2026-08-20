// Memory scanner validation tests.
//
// Tests the heuristic scoring, entry validation, and collection algebra
// independently of a live MTGA.exe process. Property-based tests via proptest
// verify mathematical invariants that survive any weight retuning.

use std::collections::{HashMap, HashSet};

use proptest::prelude::*;

use super::*;

// --- Builders ---
// Self-documenting constructors for .NET Dictionary<int,int> entries.
// The builder IS the format spec — if .NET changes Dictionary internals,
// update one builder and all tests adapt.

/// Build a 16-byte Dictionary.Entry: [hashCode: i32, next: i32, key: i32, value: i32]
#[allow(dead_code)]
struct EntryBuilder {
    hash_code: i32,
    next: i32,
    key: i32,
    value: i32,
}

#[allow(dead_code)]
impl EntryBuilder {
    /// Valid entry with correct hash derivation.
    fn new(key: i32, value: i32) -> Self {
        Self {
            hash_code: key & 0x7FFF_FFFF,
            next: -1,
            key,
            value,
        }
    }

    fn next(mut self, n: i32) -> Self {
        self.next = n;
        self
    }

    fn corrupt_hash(mut self, h: i32) -> Self {
        self.hash_code = h;
        self
    }

    fn to_bytes(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..4].copy_from_slice(&self.hash_code.to_le_bytes());
        buf[4..8].copy_from_slice(&self.next.to_le_bytes());
        buf[8..12].copy_from_slice(&self.key.to_le_bytes());
        buf[12..16].copy_from_slice(&self.value.to_le_bytes());
        buf
    }
}

/// Build a multi-entry buffer for region-level tests.
#[allow(dead_code)]
struct RegionBuilder {
    data: Vec<u8>,
}

#[allow(dead_code)]
impl RegionBuilder {
    fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Append a valid entry.
    fn card(mut self, key: i32, value: i32) -> Self {
        self.data.extend_from_slice(&EntryBuilder::new(key, value).to_bytes());
        self
    }

    /// Append N garbage entries (0xFF fill — fails every validation check).
    fn garbage(mut self, count: usize) -> Self {
        for _ in 0..count {
            self.data.extend_from_slice(&[0xFF; ENTRY_SIZE]);
        }
        self
    }

    /// Append an entry with wrong hash (valid key/value but hash corruption).
    fn bad_hash(mut self, key: i32, value: i32) -> Self {
        self.data.extend_from_slice(
            &EntryBuilder::new(key, value).corrupt_hash(0xDEAD).to_bytes(),
        );
        self
    }

    fn build(self) -> Vec<u8> {
        self.data
    }
}

/// Standard test ID set — 200 card IDs from 70000..70200.
/// Range chosen to be plausible MTGA grpIds without colliding with real data.
fn test_valid_ids() -> HashSet<i32> {
    (70_000..70_200).collect()
}

/// Small anchor set — subset of test_valid_ids.
#[allow(dead_code)]
fn test_anchor_ids() -> HashSet<i32> {
    (70_000..70_010).collect()
}

// ============================================================================
// Group 1: Format canaries
// ============================================================================
// These exist to fail loudly if .NET Dictionary internals change.

#[test]
fn entry_size_is_16_bytes() {
    // .NET Dictionary.Entry = hashCode(i32) + next(i32) + key(i32) + value(i32) = 16
    assert_eq!(ENTRY_SIZE, 16);
    assert_eq!(std::mem::size_of::<i32>() * 4, ENTRY_SIZE);
}

#[test]
fn hash_algorithm_is_key_and_mask() {
    // .NET Dictionary uses key.GetHashCode() & 0x7FFFFFFF for non-negative hash.
    // For int keys, GetHashCode() == the int itself.
    assert_eq!(42 & 0x7FFF_FFFF, 42);
    assert_eq!(0 & 0x7FFF_FFFF, 0);
    assert_eq!(i32::MAX & 0x7FFF_FFFF, i32::MAX);
    // Negative key: high bit stripped
    assert_eq!((-1i32) & 0x7FFF_FFFF, 0x7FFF_FFFF);
    assert_eq!(i32::MIN & 0x7FFF_FFFF, 0);
}

// ============================================================================
// Group 2: validate_entry — corruption per field
// ============================================================================

#[test]
fn valid_entry_accepted() {
    let ids = test_valid_ids();
    let key = 70_042;
    let hash = key & 0x7FFF_FFFF;
    assert!(validate_entry(hash, -1, key, 4, &ids));
}

#[test]
fn value_zero_rejected() {
    let ids = test_valid_ids();
    let key = 70_001;
    let hash = key & 0x7FFF_FFFF;
    assert!(!validate_entry(hash, -1, key, 0, &ids));
}

#[test]
fn value_over_max_rejected() {
    let ids = test_valid_ids();
    let key = 70_001;
    let hash = key & 0x7FFF_FFFF;
    assert!(!validate_entry(hash, -1, key, MAX_QUANTITY + 1, &ids));
}

#[test]
fn value_negative_rejected() {
    let ids = test_valid_ids();
    let key = 70_001;
    let hash = key & 0x7FFF_FFFF;
    assert!(!validate_entry(hash, -1, key, -1, &ids));
}

#[test]
fn wrong_hash_rejected() {
    let ids = test_valid_ids();
    let key = 70_001;
    // Correct would be key & 0x7FFF_FFFF = 70_001
    assert!(!validate_entry(0xDEAD, -1, key, 4, &ids));
}

#[test]
fn next_idx_too_large_rejected() {
    let ids = test_valid_ids();
    let key = 70_001;
    let hash = key & 0x7FFF_FFFF;
    assert!(!validate_entry(hash, 100_001, key, 4, &ids));
}

#[test]
fn unknown_card_id_rejected() {
    let ids = test_valid_ids();
    // 99_999 not in 70_000..70_200
    let key = 99_999;
    let hash = key & 0x7FFF_FFFF;
    assert!(!validate_entry(hash, -1, key, 4, &ids));
}

#[test]
fn next_idx_boundary_values() {
    let ids = test_valid_ids();
    let key = 70_050;
    let hash = key & 0x7FFF_FFFF;
    // -1 is the terminal sentinel
    assert!(validate_entry(hash, -1, key, 1, &ids));
    // 0 is a valid array index
    assert!(validate_entry(hash, 0, key, 1, &ids));
    // 100_000 is the maximum accepted
    assert!(validate_entry(hash, 100_000, key, 1, &ids));
    // -2 is invalid (negative but not -1)
    assert!(!validate_entry(hash, -2, key, 1, &ids));
}

// ============================================================================
// Group 3: validate_entry — proptest properties
// ============================================================================

proptest! {
    #[test]
    fn valid_entries_always_pass(
        id_offset in 0..200i32,
        value in 1..=MAX_QUANTITY,
        next in prop_oneof![Just(-1i32), 0..=100_000i32],
    ) {
        let ids = test_valid_ids();
        let key = 70_000 + id_offset;
        let hash = key & 0x7FFF_FFFF;
        prop_assert!(validate_entry(hash, next, key, value, &ids));
    }

    #[test]
    fn entries_with_bad_value_range_always_fail(
        id_offset in 0..200i32,
        // Values outside [1, MAX_QUANTITY]
        value in prop_oneof![
            (i32::MIN..=0i32),
            ((MAX_QUANTITY + 1)..=i32::MAX),
        ],
    ) {
        let ids = test_valid_ids();
        let key = 70_000 + id_offset;
        let hash = key & 0x7FFF_FFFF;
        prop_assert!(!validate_entry(hash, -1, key, value, &ids));
    }
}

// ============================================================================
// Group 4: CandidateBlock::density + score
// ============================================================================

/// Helper to build a CandidateBlock with given parameters.
fn make_candidate(
    valid: usize,
    total: usize,
    anchor_hits: usize,
    anchor_total: usize,
) -> CandidateBlock {
    CandidateBlock {
        address: 0x1000,
        entries: (0..valid).map(|i| (70_000 + i as i32, 4)).collect(),
        total_entries: total,
        anchor_hits,
        anchor_total,
    }
}

#[test]
fn density_zero_total_returns_zero() {
    let block = CandidateBlock {
        address: 0,
        entries: Vec::new(),
        total_entries: 0,
        anchor_hits: 0,
        anchor_total: 0,
    };
    assert_eq!(block.density(), 0.0);
}

proptest! {
    #[test]
    fn density_bounded_property(
        valid in 0..10_000usize,
        extra in 0..10_000usize,
    ) {
        let total = valid + extra;
        let block = make_candidate(valid, total.max(1), 0, 0);
        let d = block.density();
        prop_assert!(d >= 0.0);
        prop_assert!(d <= 1.0);
    }
}

#[test]
fn more_entries_scores_higher() {
    let big = make_candidate(500, 600, 0, 0);
    let small = make_candidate(200, 300, 0, 0);
    assert!(big.score() > small.score());
}

#[test]
fn higher_density_scores_higher() {
    // 400/500 = 0.8 density
    let dense = make_candidate(400, 500, 0, 0);
    // 400/900 ≈ 0.44 density (same valid count, more total)
    let sparse = make_candidate(400, 900, 0, 0);
    assert!(dense.score() > sparse.score());
}

#[test]
fn anchor_matches_boost_score() {
    let with_anchors = make_candidate(300, 400, 8, 10);
    let no_anchors = make_candidate(300, 400, 0, 10);
    assert!(with_anchors.score() > no_anchors.score());
}

proptest! {
    #[test]
    fn score_non_negative_property(
        valid in 0..5_000usize,
        extra in 0..5_000usize,
        anchor_hits in 0..100usize,
        anchor_total in 1..100usize,
    ) {
        let total = (valid + extra).max(1);
        let hits = anchor_hits.min(anchor_total);
        let block = make_candidate(valid, total, hits, anchor_total);
        prop_assert!(block.score() >= 0.0);
    }

    #[test]
    fn score_monotonic_in_entry_count_property(
        base_valid in 1..2_000usize,
        extra_entries in 1..500usize,
        total_padding in 0..1_000usize,
    ) {
        let total = base_valid + extra_entries + total_padding;
        let small = make_candidate(base_valid, total, 0, 0);
        let big = make_candidate(base_valid + extra_entries, total, 0, 0);
        // More valid entries in same total → higher score (count goes up, density goes up)
        prop_assert!(big.score() >= small.score());
    }

    #[test]
    fn score_monotonic_in_anchor_ratio_property(
        valid in 100..1_000usize,
        total in 100..2_000usize,
        low_hits in 0..50usize,
        boost in 1..50usize,
        anchor_total in 50..100usize,
    ) {
        let total = total.max(valid);
        let low = low_hits.min(anchor_total);
        let high = (low + boost).min(anchor_total);
        let low_block = make_candidate(valid, total, low, anchor_total);
        let high_block = make_candidate(valid, total, high, anchor_total);
        prop_assert!(high_block.score() >= low_block.score());
    }
}

// ============================================================================
// Group 5: finalize_run — threshold boundaries
// ============================================================================

/// Helper: build a run of valid entries and call finalize_run.
/// Returns the candidates vec after finalization.
fn run_finalize(
    entries: Vec<(i32, i32)>,
    total_entries: usize,
    anchor_ids: &HashSet<i32>,
) -> (Vec<CandidateBlock>, Vec<(i32, i32)>) {
    let mut candidates = Vec::new();
    let mut run = entries;
    finalize_run(&mut candidates, &mut run, total_entries, 0x5000, anchor_ids);
    let remaining = run;
    (candidates, remaining)
}

#[test]
fn run_at_threshold_creates_candidate() {
    let anchor_ids = HashSet::new();
    // Exactly MIN_VALID_ENTRIES valid entries, density = 1.0 (total == valid)
    let entries: Vec<(i32, i32)> = (0..MIN_VALID_ENTRIES)
        .map(|i| (70_000 + i as i32, 4))
        .collect();
    let total = entries.len();
    let (candidates, _) = run_finalize(entries, total, &anchor_ids);
    assert_eq!(candidates.len(), 1);
}

#[test]
fn run_below_threshold_rejected() {
    let anchor_ids = HashSet::new();
    let entries: Vec<(i32, i32)> = (0..(MIN_VALID_ENTRIES - 1))
        .map(|i| (70_000 + i as i32, 4))
        .collect();
    let total = entries.len();
    let (candidates, remaining) = run_finalize(entries, total, &anchor_ids);
    assert_eq!(candidates.len(), 0);
    // Run is cleared after rejection
    assert!(remaining.is_empty());
}

#[test]
fn density_at_boundary_accepted() {
    let anchor_ids = HashSet::new();
    // Need density == MIN_DENSITY exactly.
    // density = valid / total. With valid=MIN_VALID_ENTRIES, total = valid / MIN_DENSITY
    let valid = MIN_VALID_ENTRIES;
    let total = (valid as f64 / MIN_DENSITY) as usize;
    // Verify our constructed density is >= MIN_DENSITY (floating point)
    let density = valid as f64 / total as f64;
    assert!(density >= MIN_DENSITY, "test setup: density={density} < MIN_DENSITY={MIN_DENSITY}");

    let entries: Vec<(i32, i32)> = (0..valid)
        .map(|i| (70_000 + i as i32, 4))
        .collect();
    let (candidates, _) = run_finalize(entries, total, &anchor_ids);
    assert_eq!(candidates.len(), 1);
}

#[test]
fn density_below_boundary_rejected() {
    let anchor_ids = HashSet::new();
    let valid = MIN_VALID_ENTRIES;
    // total large enough that density falls just below MIN_DENSITY
    let total = (valid as f64 / MIN_DENSITY) as usize + 2;
    let density = valid as f64 / total as f64;
    assert!(density < MIN_DENSITY, "test setup: density={density} >= MIN_DENSITY={MIN_DENSITY}");

    let entries: Vec<(i32, i32)> = (0..valid)
        .map(|i| (70_000 + i as i32, 4))
        .collect();
    let (candidates, _) = run_finalize(entries, total, &anchor_ids);
    assert_eq!(candidates.len(), 0);
}

#[test]
fn anchor_hits_computed_correctly() {
    let anchor_ids: HashSet<i32> = [70_000, 70_001, 70_002, 70_050, 70_099].into();
    // Build a run that contains some anchor IDs and some non-anchor
    let entries: Vec<(i32, i32)> = (0..MIN_VALID_ENTRIES)
        .map(|i| (70_000 + i as i32, 4))
        .collect();
    let total = entries.len();
    let (candidates, _) = run_finalize(entries, total, &anchor_ids);
    assert_eq!(candidates.len(), 1);
    // All 5 anchor IDs are in range 70_000..70_000+MIN_VALID_ENTRIES (100),
    // so all should be found
    assert_eq!(candidates[0].anchor_hits, 5);
    assert_eq!(candidates[0].anchor_total, 5);
}

#[test]
fn empty_anchor_set_zero_hits() {
    let anchor_ids = HashSet::new();
    let entries: Vec<(i32, i32)> = (0..MIN_VALID_ENTRIES)
        .map(|i| (70_000 + i as i32, 4))
        .collect();
    let total = entries.len();
    let (candidates, _) = run_finalize(entries, total, &anchor_ids);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].anchor_hits, 0);
    assert_eq!(candidates[0].anchor_total, 0);
}

#[test]
fn rejected_run_is_cleared() {
    let anchor_ids = HashSet::new();
    // Too few entries
    let entries: Vec<(i32, i32)> = (0..5).map(|i| (70_000 + i as i32, 4)).collect();
    let total = entries.len();
    let (_, remaining) = run_finalize(entries, total, &anchor_ids);
    assert!(remaining.is_empty());
}

// ============================================================================
// Group 6: build_collection
// ============================================================================

#[test]
fn dedup_takes_max_quantity() {
    let block = CandidateBlock {
        address: 0,
        entries: vec![(70_001, 2), (70_001, 4), (70_001, 3)],
        total_entries: 3,
        anchor_hits: 0,
        anchor_total: 0,
    };
    let non_collectible = HashSet::new();
    let col = build_collection(&block, &non_collectible);
    assert_eq!(col.get(&70_001), Some(&4));
}

#[test]
fn non_collectible_excluded() {
    let block = CandidateBlock {
        address: 0,
        entries: vec![(70_001, 4), (70_002, 3), (70_003, 1)],
        total_entries: 3,
        anchor_hits: 0,
        anchor_total: 0,
    };
    let non_collectible: HashSet<i32> = [70_002].into();
    let col = build_collection(&block, &non_collectible);
    assert_eq!(col.len(), 2);
    assert!(col.contains_key(&70_001));
    assert!(!col.contains_key(&70_002));
    assert!(col.contains_key(&70_003));
}

#[test]
fn empty_block_returns_empty() {
    let block = CandidateBlock {
        address: 0,
        entries: Vec::new(),
        total_entries: 0,
        anchor_hits: 0,
        anchor_total: 0,
    };
    let col = build_collection(&block, &HashSet::new());
    assert!(col.is_empty());
}

#[test]
fn all_non_collectible_returns_empty() {
    let block = CandidateBlock {
        address: 0,
        entries: vec![(70_001, 4), (70_002, 3)],
        total_entries: 2,
        anchor_hits: 0,
        anchor_total: 0,
    };
    let non_collectible: HashSet<i32> = [70_001, 70_002].into();
    let col = build_collection(&block, &non_collectible);
    assert!(col.is_empty());
}

proptest! {
    #[test]
    fn dedup_max_wins_property(
        card_id in 70_000..70_100i32,
        qty_a in 1..=MAX_QUANTITY,
        qty_b in 1..=MAX_QUANTITY,
    ) {
        let block = CandidateBlock {
            address: 0,
            entries: vec![(card_id, qty_a), (card_id, qty_b)],
            total_entries: 2,
            anchor_hits: 0,
            anchor_total: 0,
        };
        let col = build_collection(&block, &HashSet::new());
        prop_assert_eq!(col[&card_id], qty_a.max(qty_b));
    }
}

// ============================================================================
// Group 7: diff_collections
// ============================================================================

#[test]
fn diff_detects_added_cards() {
    let old: HashMap<i32, i32> = [(70_001, 4)].into();
    let new: HashMap<i32, i32> = [(70_001, 4), (70_002, 2)].into();
    let diff = diff_collections(&old, &new);
    assert_eq!(diff.added.len(), 1);
    assert!(diff.added.contains(&(70_002, 2)));
    assert!(diff.increased.is_empty());
    assert!(diff.removed.is_empty());
}

#[test]
fn diff_detects_increased_quantity() {
    let old: HashMap<i32, i32> = [(70_001, 2)].into();
    let new: HashMap<i32, i32> = [(70_001, 4)].into();
    let diff = diff_collections(&old, &new);
    assert!(diff.added.is_empty());
    assert_eq!(diff.increased.len(), 1);
    assert!(diff.increased.contains(&(70_001, 2, 4)));
    assert!(diff.removed.is_empty());
}

#[test]
fn diff_detects_removed_cards() {
    let old: HashMap<i32, i32> = [(70_001, 4), (70_002, 2)].into();
    let new: HashMap<i32, i32> = [(70_001, 4)].into();
    let diff = diff_collections(&old, &new);
    assert!(diff.added.is_empty());
    assert!(diff.increased.is_empty());
    assert_eq!(diff.removed.len(), 1);
    assert!(diff.removed.contains(&(70_002, 2)));
}

#[test]
fn diff_identical_is_empty() {
    let data: HashMap<i32, i32> = [(70_001, 4), (70_002, 2)].into();
    let diff = diff_collections(&data, &data);
    assert!(diff.added.is_empty());
    assert!(diff.increased.is_empty());
    assert!(diff.removed.is_empty());
}

proptest! {
    #[test]
    fn diff_from_empty_is_all_added(
        entries in proptest::collection::hash_map(70_000..70_500i32, 1..=MAX_QUANTITY, 0..50),
    ) {
        let empty: HashMap<i32, i32> = HashMap::new();
        let diff = diff_collections(&empty, &entries);
        prop_assert_eq!(diff.added.len(), entries.len());
        prop_assert!(diff.increased.is_empty());
        prop_assert!(diff.removed.is_empty());
    }

    #[test]
    fn diff_to_empty_is_all_removed(
        entries in proptest::collection::hash_map(70_000..70_500i32, 1..=MAX_QUANTITY, 0..50),
    ) {
        let empty: HashMap<i32, i32> = HashMap::new();
        let diff = diff_collections(&entries, &empty);
        prop_assert!(diff.added.is_empty());
        prop_assert!(diff.increased.is_empty());
        prop_assert_eq!(diff.removed.len(), entries.len());
    }

    #[test]
    fn diff_self_is_empty(
        entries in proptest::collection::hash_map(70_000..70_500i32, 1..=MAX_QUANTITY, 0..50),
    ) {
        let diff = diff_collections(&entries, &entries);
        prop_assert!(diff.added.is_empty());
        prop_assert!(diff.increased.is_empty());
        prop_assert!(diff.removed.is_empty());
    }
}

// ============================================================================
// Group 8: cross_validate
// ============================================================================

#[test]
fn cross_validate_partial_overlap() {
    let a: HashMap<i32, i32> = [(70_001, 4), (70_002, 2), (70_003, 1)].into();
    let b: HashMap<i32, i32> = [(70_002, 2), (70_003, 1), (70_004, 3)].into();
    let cv = cross_validate(&a, &b);
    assert_eq!(cv.shared_ids, 2); // 70_002, 70_003
    assert_eq!(cv.quantity_matches, 2);
    assert_eq!(cv.quantity_mismatches, 0);
    assert_eq!(cv.only_in_a, 1); // 70_001
    assert_eq!(cv.only_in_b, 1); // 70_004
}

#[test]
fn cross_validate_quantity_mismatch() {
    let a: HashMap<i32, i32> = [(70_001, 4), (70_002, 2)].into();
    let b: HashMap<i32, i32> = [(70_001, 3), (70_002, 2)].into();
    let cv = cross_validate(&a, &b);
    assert_eq!(cv.shared_ids, 2);
    assert_eq!(cv.quantity_matches, 1); // 70_002
    assert_eq!(cv.quantity_mismatches, 1); // 70_001
}

proptest! {
    #[test]
    fn cross_validate_self_is_perfect(
        entries in proptest::collection::hash_map(70_000..70_500i32, 1..=MAX_QUANTITY, 0..50),
    ) {
        let cv = cross_validate(&entries, &entries);
        prop_assert_eq!(cv.shared_ids, entries.len());
        prop_assert_eq!(cv.quantity_matches, entries.len());
        prop_assert_eq!(cv.quantity_mismatches, 0);
        prop_assert_eq!(cv.only_in_a, 0);
        prop_assert_eq!(cv.only_in_b, 0);
    }

    #[test]
    fn cross_validate_disjoint_shares_nothing(
        a_entries in proptest::collection::hash_map(70_000..70_100i32, 1..=MAX_QUANTITY, 0..30),
        b_entries in proptest::collection::hash_map(80_000..80_100i32, 1..=MAX_QUANTITY, 0..30),
    ) {
        let cv = cross_validate(&a_entries, &b_entries);
        prop_assert_eq!(cv.shared_ids, 0);
        prop_assert_eq!(cv.quantity_matches, 0);
        prop_assert_eq!(cv.quantity_mismatches, 0);
        prop_assert_eq!(cv.only_in_a, a_entries.len());
        prop_assert_eq!(cv.only_in_b, b_entries.len());
    }
}
