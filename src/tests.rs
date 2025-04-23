use super::CutoffList;

use index_list::Index;
use quickcheck::{Arbitrary, Gen, TestResult};
use quickcheck_macros::quickcheck;

#[derive(Debug, Clone, Copy)]
enum Op {
    InsertFirst,
    InsertLast,
    ShiftToFront(usize),
    Remove(usize),
}

impl Arbitrary for Op {
    fn arbitrary(g: &mut Gen) -> Self {
        match g.choose(&[0, 1, 2, 3]).unwrap() {
            0 => Op::InsertFirst,
            1 => Op::InsertLast,
            2 => Op::ShiftToFront(usize::arbitrary(g)),
            3 => Op::Remove(usize::arbitrary(g)),
            _ => unreachable!(),
        }
    }
}

fn get_indices<T>(list: &CutoffList<T>) -> Vec<Index> {
    let mut indices = Vec::new();
    let mut idx = list.first_index();
    while idx.is_some() {
        indices.push(idx);
        idx = list.next_index(idx);
    }
    indices
}

#[quickcheck]
fn qc_cutoff_list_all_ops(cutoffs: Vec<usize>, ops: Vec<Op>) -> TestResult {
    let mut unique_cutoffs = cutoffs;
    unique_cutoffs.sort_unstable();
    unique_cutoffs.dedup();
    const MAX_CUTOFF_VAL: usize = 150;
    const MAX_CUTOFF_COUNT: usize = 40;
    unique_cutoffs.retain(|&c| c <= MAX_CUTOFF_VAL);
    unique_cutoffs.truncate(MAX_CUTOFF_COUNT);
    const MAX_OPS: usize = 250;
    let limited_ops = &ops[..ops.len().min(MAX_OPS)];

    let mut list: CutoffList<()> = CutoffList::new(unique_cutoffs);

    for op in limited_ops {
        match op {
            Op::InsertFirst => {
                list.insert_first(());
            }
            Op::InsertLast => {
                list.insert_last(());
            }
            Op::ShiftToFront(rand_val) => {
                let valid_indices = get_indices(&list);
                if !valid_indices.is_empty() {
                    let index_to_shift = valid_indices[rand_val % valid_indices.len()];
                    list.shift_to_front(index_to_shift);
                }
            }
            Op::Remove(rand_val) => {
                let valid_indices = get_indices(&list);
                if !valid_indices.is_empty() {
                    let index_to_remove = valid_indices[rand_val % valid_indices.len()];
                    list.remove(index_to_remove);
                }
            }
        }
    }

    list.validate();

    TestResult::passed()
}
