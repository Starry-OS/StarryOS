pub mod map;
pub mod prog;
pub mod tansform;

use alloc::collections::btree_map::BTreeMap;

use kbpf_basic::helper::RawBPFHelperFn;
use lazyinit::LazyInit;

use crate::bpf::tansform::EbpfKernelAuxiliary;

pub static BPF_HELPER_FUN_SET: LazyInit<BTreeMap<u32, RawBPFHelperFn>> = LazyInit::new();

pub fn init_bpf() {
    let set = kbpf_basic::helper::init_helper_functions::<EbpfKernelAuxiliary>();
    BPF_HELPER_FUN_SET.init_once(set);
}
