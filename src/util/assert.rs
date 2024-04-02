/// Test if `ptr` is aligned to a multiple of `align`
pub fn aligned_to<T>(ptr: *mut T, align: usize) -> bool {
    ptr as usize % align == 0
}

/// Test if this pointer is non-null (`true` if it is non-null)
pub fn non_null<T>(ptr: *mut T) -> bool
where
    T: ?Sized,
{
    !ptr.is_null()
}
