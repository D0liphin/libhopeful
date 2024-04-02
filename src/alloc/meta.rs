//! A fast ptr -> ptr map

use std::alloc::Layout;
use std::cmp::Ordering;
// use std::borrow::Borrow;
use std::collections::HashSet;
use std::hint::unreachable_unchecked;
// use std::marker::PhantomData;
use std::{collections::HashMap, sync::Mutex};

use std::{
    any::type_name,
    // io::{BufWriter, Write},
    // marker::PhantomData,
    mem::{align_of, size_of},
};

// use crate::alloc::global::ALLOC_META_INDEX;


#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AllocId {
    address: usize,
    size: usize,
    align: usize,
}

impl AllocId {
    /// # Safety
    ///
    /// This must point to a *possible* allocation. For example, an allocation
    /// that is at `address` `usize::MAX` with `size` `usize::MAX` is not
    /// possible.
    pub unsafe fn from_alloc<T>(address: *mut T, size: usize, align: usize) -> Self {
        debug_assert!((address as usize).checked_add(size).is_some());
        Self {
            address: address as usize,
            size,
            align,
        }
    }

    /// Tests if `ptr` is interior to this allocation.
    ///
    /// # Notes
    ///
    /// Actually, this produces exactly the same text as the naive
    /// implementation... Will leave as this to investigate in the future how
    /// we can direct the assembly to do something a little bit better? maybe?
    pub fn test_interior_ptr(self, ptr: usize) -> Ordering {
        // SAFETY:
        //   - We can assume the alloc ID has been constructed correctly (as
        //     required by sole constructor).
        //   - No functions take `&mut self`, so it cannot be mutated.
        let (start, end) = (self.address, unsafe {
            self.address.unchecked_add(self.size)
        });
        // So... we can do this without branching at all. A pointer is either
        // too low or too high, never both.
        //
        // |   ptr   | too_low | too_high |   Q  |
        // |:-------:|:-------:|:--------:|:----:|
        // | < start |  0xff   |    0     | 0xff | // 0xff is -1
        // | >= end  |   1     |    0     |  1   |
        // | else    |   0     |    0     |  0   |
        let too_low = -((ptr < start) as i8);
        let too_high = (ptr >= end) as i8;
        match too_low | too_high {
            -1 => Ordering::Less,
            0 => Ordering::Equal,
            1 => Ordering::Greater,
            _ => unsafe { unreachable_unchecked() },
        }
    }
}

/// Describes `AllocMeta` that may or may not be available. Obviously, we want
/// this to contain DWARF type info in the future, hence why I include the
/// lifetime (which should really be static, but whatever).
#[derive(PartialEq, Eq, Hash, Debug)]
pub struct AllocMeta {
    // alloc_id: AllocId,
    // align: usize,
}

pub(crate) struct AMINode {
    id: AllocId,
    /// First bit is reserved to mark null (0 -> null, 1 -> some)
    /// Second bit is reserved to mark color (0 -> Red, 1 -> Black)
    greater: usize,
    /// First bit is reserved to mark null (0 -> null, 1 -> some)
    /// Second bit is reserved, but is not used for anything
    lesser: usize,
}

#[derive(Debug)]
#[repr(u8)]
pub(crate) enum Color {
    Red,
    Black,
}

impl AMINode {
    fn as_index(index: usize) -> Option<usize> {
        if (index & !(usize::MAX >> 1)) == 0 {
            None
        } else {
            Some(index & (usize::MAX >> 2))
        }
    }

    pub(crate) fn greater(&self) -> Option<usize> {
        Self::as_index(self.greater)
    }

    pub(crate) fn lesser(&self) -> Option<usize> {
        Self::as_index(self.greater)
    }

    /// Get the color of this node
    pub(crate) fn color(&self) -> Color {
        let is_black = self.greater & (1 << (size_of::<usize>() - 2)) == 0;
        if is_black {
            Color::Black
        } else {
            Color::Red
        }
    }
}



pub struct AllocMetaIndex {
    heap: Vec<AMINode>,
}

impl AllocMetaIndex {
    pub const fn new() -> Self {
        Self { heap: Vec::new() }
    }

    /// Register an allocation with unknown [`AllocMeta`].
    pub fn insert_unknown(&self, id: AllocId) {
        todo!()
    }

    /// Register an allocation with a known type. [`AllocMeta`] is derived,
    /// interened and leaked as appropriate.
    pub fn insert<T>(&self, id: AllocId) {
        todo!()
    }

    /// Get the metadata and allocation ID for some pointer.
    ///
    /// # Returns
    ///
    /// - `None` if the pointer is not registered
    /// - `Some(alloc_id, meta)` otherwise. `meta` may be `None`, because not
    ///   all allocations can be guaranteed to have ascribed metadata.
    pub fn get(&self, ptr: usize) -> Option<(AllocId, Option<&'static AllocMeta>)> {
        todo!()
    }
}
