use std::{
    cell::{Cell, UnsafeCell},
    mem::MaybeUninit,
    ops::Deref,
    sync::Mutex,
};

use crate::util::{hint::cold, print::putstr};

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LazyLockState {
    /// Signifies this lock is uninitialized
    Uninit,
    /// Signifies this lock is initialized
    Init,
    /// Signifies this lock is mid initialization. This is only really useful
    /// when checking the state inside the initializer...
    Initializing,
}

/// A LazyLock that let's you poll its initialization state (including mid-init)
pub struct LazyLock<T, F> {
    cell: UnsafeCell<MaybeUninit<T>>,
    lock: Mutex<()>,
    state: Cell<LazyLockState>,
    init: F,
}

unsafe impl<T, F> Sync for LazyLock<T, F> where F: Sync {}

impl<T, F> LazyLock<T, F>
where
    // TODO: Remove the `Copy` bound, in favour of unsafe MU impl
    F: FnOnce() -> T + Copy,
{
    pub const fn new(init: F) -> Self {
        Self {
            cell: UnsafeCell::new(MaybeUninit::uninit()),
            lock: Mutex::new(()),
            state: Cell::new(LazyLockState::Uninit),
            init,
        }
    }

    pub fn state(lock: &Self) -> LazyLockState {
        lock.state.get()
    }

    pub unsafe fn assume_init(&self) -> &T {
        // This borrow is not needless
        #[allow(clippy::needless_borrow)]
        (&*self.cell.get()).assume_init_ref()
    }

    pub fn initialize(&self) -> &T {
        if LazyLock::state(self) != LazyLockState::Init {
            putstr(c"initialize");
            cold(|| {
                // I don't think it's possible to get poison with this setup.
                // We only lock this mutex once, and it is right here... There
                // are no panics here!
                // TODO: Justify separating `Mutex` from data
                // TODO: Justify non-atmoic `self.state`
                let _guard = self.lock.lock().expect("we will never get a poision value");
                self.state.set(LazyLockState::Initializing);
                unsafe { *self.cell.get() = MaybeUninit::new((self.init)()) };
                self.state.set(LazyLockState::Init);
            })
        }
        // TODO: Safety comments
        unsafe { self.assume_init() }
    }
}

impl<T, F> Deref for LazyLock<T, F>
where
    F: FnOnce() -> T + Copy,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.initialize()
    }
}
