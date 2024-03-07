/// mark some stuff as #[cold]
#[cold]
pub fn cold<R, F: Fn() -> R>(f: F) -> R {
    f()
}
