use std::{
    alloc::Allocator,
    fmt,
    fs::File,
    io::{self, Read, Write},
    mem::ManuallyDrop,
    path::Path,
};

use hashbrown::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

use crate::alloc::tracing::{
    AllocId, FindPointerMethod, FlatAllocator, HexDump, PtrMeta, TracingAlloc,
};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocEdge {
    ptr_meta: PtrMeta,
}

impl fmt::Debug for AllocEdge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AllocEdge(-> {:?})", self.ptr_meta.id.ptr)
    }
}

impl AllocEdge {
    pub const fn new(ptr_meta: PtrMeta) -> Self {
        Self { ptr_meta }
    }

    pub const fn alloc_id(&self) -> &AllocId {
        &self.ptr_meta.id
    }
}

/// This is a no-op
///
/// # Safety
///
/// - `&T` and `&U` must be safely castable back and forth
unsafe fn cast_vec<T, U>(v: Vec<T>) -> Vec<U> {
    let mut v = ManuallyDrop::new(v);
    Vec::from_raw_parts(v.as_mut_ptr() as *mut U, v.len(), v.capacity())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AllocNode {
    pub(crate) alloc_id: AllocId,
    pub(crate) hex_dump: HexDump,
    pub(crate) children: Vec<AllocEdge>,
}

#[derive(Debug)]
pub struct AllocGraph {
    pub(crate) graph: HashMap<AllocId, AllocNode>,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum AllocGraphFromFileError {
    IoError(io::Error),
    SerdeJsonError(serde_json::error::Error),
}

impl AllocGraph {
    fn new() -> Self {
        Self {
            graph: HashMap::new(),
        }
    }

    /// Get all the root nodes of this graph (that means that they do not appear
    /// as the child of any node). This operation is O(N + E)
    pub fn roots(&self) -> HashSet<AllocId> {
        let mut result = HashSet::from_iter(self.graph.keys().copied());
        for (_, node) in self.graph.iter() {
            for edge in node.children.iter() {
                result.remove(edge.alloc_id());
            }
        }
        result
    }

    /// Construct a new AllocGraph for a given allocation -- this allocation is
    /// represented by `ptr`, which can point anywhere inside the allocation.
    pub fn from_alloc<A>(
        global: &TracingAlloc<A>,
        ptr: *const (),
        find_ptr_method: FindPointerMethod,
    ) -> Self
    where
        A: Allocator + FlatAllocator,
    {
        let Some(alloc_id) = global.find(ptr) else {
            return Self::new();
        };
        let mut graph = Self::new();
        // SAFETY: TODO
        unsafe {
            graph.init_from_alloc(global, alloc_id, find_ptr_method);
        }
        graph
    }

    pub fn from_sized<T, A>(
        global: &TracingAlloc<A>,
        value: &T,
        find_ptr_method: FindPointerMethod,
    ) -> Self
    where
        A: Allocator + FlatAllocator,
    {
        let alloc_id = AllocId::from_sized(value);
        let mut graph = Self::new();
        // SAFETY: TODO
        unsafe {
            graph.init_from_alloc(global, alloc_id, find_ptr_method);
        }
        graph
    }

    /// # Safety
    ///
    /// - GHL must not be acquired
    /// - `AllocId` must point to memory that is guaranteed to be readable with
    ///   `memcpy_maybe_garbage()` for the entire duration of this function
    unsafe fn init_from_alloc<A>(
        &mut self,
        global: &TracingAlloc<A>,
        alloc_id: AllocId,
        find_ptr_method: FindPointerMethod,
    ) where
        A: Allocator + FlatAllocator,
    {
        let ptrs = alloc_id.find_pointers(global, find_ptr_method);
        let children = unsafe { cast_vec(ptrs.clone()) };
        let node = AllocNode {
            alloc_id,
            // SAFETY: requirement passed to caller
            hex_dump: unsafe { alloc_id.read_unchecked() },
            children,
        };
        self.graph.insert(alloc_id, node);
        for ptr in ptrs {
            if !self.graph.contains_key(&ptr.id) {
                self.init_from_alloc(global, ptr.id, find_ptr_method);
            }
        }
    }

    pub fn kv_pairs(&self) -> Vec<(&AllocId, &AllocNode)> {
        self.graph.iter().collect()
    }

    /// TODO: Docs
    pub fn write_to_file<P>(&self, path: P) -> io::Result<()>
    where
        P: AsRef<Path>,
    {
        let mut file = File::create(path)?;
        file.write_all(
            &serde_json::to_vec(&self.kv_pairs())
                .expect("types should always be valid for serialization to json"),
        )?;
        Ok(())
    }

    /// TODO: Docs
    pub fn from_file<P>(path: P) -> Result<Self, AllocGraphFromFileError>
    where
        P: AsRef<Path>,
    {
        let mut file = File::open(path).map_err(AllocGraphFromFileError::IoError)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)
            .map_err(AllocGraphFromFileError::IoError)?;
        let kv_pairs: Vec<(AllocId, AllocNode)> =
            serde_json::from_str(&buf).map_err(AllocGraphFromFileError::SerdeJsonError)?;
        Ok(Self {
            graph: HashMap::from_iter(kv_pairs),
        })
    }
}
