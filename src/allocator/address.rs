#![forbid(
    box_pointers,
    pointer_structural_match,
    missing_docs,
    missing_debug_implementations
)]

use crate::allocator::arena::Arena;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

/// Address represents a "pointer" to data in the Arena. Address holds a raw pointer to the arena
/// for getting entities and also for freeing location.
#[derive(Clone, Debug)]
pub struct Address<T: 'static> {
    /// Generation of the address, for an Address to be not None, generation must be the same as
    /// the generation in the target location
    pub generation: usize,
    /// Index of the entities in the array
    pub index: usize,
    /// This is used to make the Rust compiler be type aware of the entity it is referencing
    pub phantom: PhantomData<&'static T>,
    /// Raw pointer to the arena, used for freeing and getting entities
    pub arena: *mut Arena,
    /// number of references one address has been copied to
    pub ref_count: Rc<RefCell<i16>>,
}

impl<T> Drop for Address<T> {
    /// implement the default drop method so Rust's default memory management works out of the box
    /// with Address. It does not free the entity in the arena if there are multiple references to
    /// it in the arena. This does not guarantee all references will be valid however, because the
    /// remove() method can free an entity while there are other references to the address
    ///
    /// SAFETY: It is assumed that arena is a valid reference for the entire runtime of the
    /// program, if this is not the case, dropping an address will cause a segfault
    fn drop(&mut self) {
        let mut v = self.ref_count.borrow_mut();
        *v -= 1;
        if *v == 0 {
            unsafe {
                let arena: &Arena = &*self.arena;
                arena.free(&self)
            };
        }
    }
}

impl<T> Address<T> {
    /// Get the entity the address is pointing to from the arena. None means the entity was freed
    /// by something else.
    ///
    /// SAFETY: It is assumed that arena is a valid reference for the entire runtime of the
    /// program, if this is not the case, dropping an address will cause a segfault
    pub fn get(&self) -> Option<&T> {
        unsafe {
            let arena: &Arena = &*self.arena;
            arena.get(&self)
        }
    }
    /// Get a mutable reference to entity the address is pointing to from the arena. None means the entity was freed
    /// by something else.
    ///
    /// SAFETY: It is assumed that arena is a valid reference for the entire runtime of the
    /// program, if this is not the case, dropping an address will cause a segfault
    pub fn get_mut(&self) -> Option<&mut T> {
        unsafe {
            let arena: &mut Arena = &mut *self.arena;
            arena.get_mut(&self)
        }
    }
    /// Get a copy of the Address without taking ownership
    pub fn copy(&self) -> Address<T> {
        *self.ref_count.borrow_mut() += 1;
        Address {
            generation: self.generation,
            index: self.index,
            phantom: PhantomData,
            arena: self.arena,
            ref_count: Rc::clone(&self.ref_count),
        }
    }

    /// Force freeing of an entity regardless of their reference count
    pub fn remove(&self) {
        let mut v = self.ref_count.borrow_mut();
        *v = -1;
        unsafe {
            let arena: &Arena = &*self.arena;
            arena.free(&self)
        };
    }
}
