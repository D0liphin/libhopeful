#[cfg(target_pointer_width = "64")]
pub mod serde_usize;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("Unsupported target arch");