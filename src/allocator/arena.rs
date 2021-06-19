/*!
# Typed Generational Allocator

[This project was inspired by this talk](https://www.youtube.com/watch?v=aKLntZcp27M)

A common pattern seen in the game dev world is to allocate a large chuck of memory on start up, and allocate chunks of
memory on that block. This goes against the RAII which is most commonly seen in C++, the language used most by game
devs.

### Why Arena Allocators

In some use uses, the program might have deep entity hierarchies. In those cases, with the standard RAII, an entity
getting deallocated could mean hundreds of calls to the OS for deallocation. Normally this is not a problem, but for
game dev where every millisecond counts, it is a huge problem. Add the many unknowns of different OSes, and modern
games where hundreds of objects get deallocated and allocated every few seconds, and Arena's start making sense.

### Generational Arena

A generational arena is a special type of arena, it holds all objects in arrays of those objects. For example you could
have:

```compile_fail
State {
    Humans: [human1, human2, human3],
    Monsters: [monster1, monster2],
}
```

This immediately has the added benefit of making the game state adhere to a data driven design. Having all objects
be in a contiguous array makes whole game updates, where every single entity must be read, extremely fast in modern CPUs
thanks to the caching of upcoming items in the list also similar DRAM optimizations.

In this structure, a "pointer" holds 3 key pieces of information(or more depending on the implementation).
Those are the index, generation, and type. Type is used to select the array in the game state (and helps the type system
be aware of each entity type). The index is simply the index of the object in that array. Finally, the generation indicates
what generation of this object is on that location. Generation is essentially used to avoid use after free bugs. In this
case since memory is never freed, all pointers are always valid, they just may point to the incorrect object. For example,
say we have a list of 2 humans, with other entities referencing them by pointers. Then, human2 gets freed, and human3
gets allocated:

```compile_fail
Humans: [human1, human2]
    | pointer_to_human2<index=1, gen=0> == human2

// human2 freed, human3 added

Humans: [human1, human3]
    | pointer_to_human2<index=1, gen=0> == human3
```

`pointer_to_human2` expects `human2`, but actually gets `human3`. Now with generations:

```compile_fail
Humans: [<gen=0, entity=human1>, <gen=0, entity=human2>]
    | pointer_to_human2<index=1, gen=0> == human2

// human2 freed, human3 added

Humans: [<gen=0, entity=human1>, <gen=1, entity=human3>]
    | pointer_to_human2<index=1, gen=0> == None
```

As you can seen, when human2 was freed, the generation value on its location in the arena got bumped, so now any
pointer with `index=1, gen=0` will know it's gone, and thanks to `Option` type, handling of this is enforced by the
compiler!

To recap, No more use after free bugs, no more segfaults, memory deallocation is simple bump of an integer, and no
OS allocation needed for new entities(provided the list of entities are initialized to a large enough size)

### This project

This project is a simple implementation of an Arena allocator, plus a small example of it in action in the `main.rs`
file, and some tests comparing the performance. A goal here was to make `Address` which is the "pointer" object in this
project, to behave just like normal Rust references. With Rust's ownership rules, this is a difficult task to accomplish.
This is due to the fact that when a pointer needs to be dropped, the pointer must mutate the arena. However, the arena
is not owned by `Address`, therefore arena needs to use raw pointer dereferencing to do so. This does mean some
`unsafe` code, but there is some explanation in the code why the unsafe could would not cause any problems, as the only
time segfaults can happen is when an `Address` is dropped after the arena itself has dropped. Since the arena is essentially
the memory allocator of the entire application, it must be initialized in the `main()` scope and will be the last thing
to be dropped in the entire program.
This module implements the arena, which is responsible for holding the data.

### Starting the arena

Arena can be created with default capacity or specific capacity, and any object or struct can be added to it.
Vecs will be created on demand
```rust
use arena_allocator::{Address, Arena};
let mut arena = Arena::default();
let mut arena_with_capacity = Arena::new(30);

struct Dog {name: String}

let dog = Dog{name: format!("Bruce")};
let dog_address = arena.allocate(dog);
let dog = dog_address.get();
assert_eq!(dog.is_some(), true);
assert_eq!(dog.unwrap().name, "Bruce");
```
values get automatically dropped as well
```rust
use arena_allocator::{Address, Arena};
let mut arena = Arena::default();

#[derive(Clone)]
struct Dog {name: String}

let dangling;
{
    let dog = Dog{name: format!("Bruce")};
    let dog_address = arena.allocate(dog);
    dangling = dog_address.clone();
}
let dog = dangling.get();
assert_eq!(dog.is_none(), true);
```
 */

#![forbid(
    box_pointers,
    pointer_structural_match,
    missing_docs,
    missing_debug_implementations
)]

use std::any::TypeId;
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::rc::Rc;

use anymap;

use super::address::Address;

static DEFAULT_CAPACITY: usize = 16;

/// Struct that holds the collection of objects
/// uses `anymap` to store the types of the objects being stored, and
/// look up the list of types
#[derive(Debug)]
pub struct Arena {
    data: anymap::Map,
    capacity: usize,
    freed_groups: Vec<u64>,
}

/// A LocationGroup is the entity that holds the array of entities and maintains a list of all
/// indexes that have been freed and can be reused
struct LocationGroup<T> {
    locations: Vec<Location<T>>,
    free_indexes: RefCell<Vec<usize>>,
    arena: *mut Arena,
    type_id_hash: u64,
}

/// Location represents an index inside the array of entities
/// the purpose of this is maintaining a generation, so that all entity lookups can know
/// if the entity is the one they are looking for
/// `RefCell` used to provide a safe way to drop values from the arena
/// without taking a mutable reference
#[derive(Clone, Debug)]
struct Location<T> {
    generation: RefCell<usize>,
    entity: T,
}

impl<T> LocationGroup<T> {
    fn new(capacity: usize, arena_ptr: *mut Arena, type_id_hash: u64) -> LocationGroup<T> {
        LocationGroup {
            locations: Vec::<Location<T>>::with_capacity(capacity),
            free_indexes: RefCell::new(Vec::<usize>::with_capacity(capacity)),
            arena: arena_ptr,
            type_id_hash,
        }
    }
}

impl<T> Drop for LocationGroup<T> {
    fn drop(&mut self) {
        unsafe {
            let arena: &mut Arena = &mut *self.arena;
            arena.freed_groups.push(self.type_id_hash)
        };
    }
}

impl Arena {
    /// Creates a new arena with a given capacity.
    /// The capacity dictates the initial size of all arrays created for each entity
    pub fn new(capacity: usize) -> Arena {
        Arena {
            data: anymap::AnyMap::new(),
            capacity,
            freed_groups: Vec::new(),
        }
    }

    /// Get a reference to the entity at a given address
    /// This method borrows the arena, so all rust borrowing rules apply to all entities in the
    /// allocator
    ///
    /// unwrap() use is safe here as it is impossible to have an Address without adding an entity
    /// for the type it is referencing. Therefore, unwrap() will never be called on None
    #[inline]
    pub fn get<T: 'static>(&self, address: &Address<T>) -> Option<&T> {
        let list = &self.data.get::<LocationGroup<T>>().unwrap().locations;
        let item = &list[address.index];
        if *item.generation.borrow() == address.generation {
            Some(&item.entity)
        } else {
            None
        }
    }

    /// Get a mutable reference to the entity at a given address
    /// This method borrows the arena, so all rust borrowing rules apply to all entities in the
    /// allocator
    /// prefer Address.get_mut() to this for mutations that are deeply nested in already borrowed
    /// entities. Rust's borrow rules are incompatible with the idea of arena allocation
    /// where all objects live forever therefore unsafe code of Address.get_mut() is necessary
    ///
    /// unwrap() use is safe here as it is impossible to have an Address without adding an entity
    /// for the type it is referencing. Therefore, unwrap() will never be called on None
    #[inline]
    pub fn get_mut<T: 'static>(&mut self, address: &Address<T>) -> Option<&mut T> {
        let list = &mut self.data.get_mut::<LocationGroup<T>>().unwrap().locations;
        let item = &mut list[address.index];
        if *item.generation.borrow() == address.generation {
            Some(&mut item.entity)
        } else {
            None
        }
    }

    /// Adds a new entity to the arena and returns the address to that entity
    #[inline]
    pub fn allocate<T: 'static>(&mut self, v: T) -> Address<T> {
        let self_ptr = self as *mut Arena;
        let group = match self.data.get_mut::<LocationGroup<T>>() {
            Some(v) => v,
            None => {
                // This hash value is used to keep track of what location groups have been freed.
                // This is used to avoid use after free bugs when the arena is being de allocated.
                // sometimes some of `Address` entities can cause segfault if they get dropped
                // after their location group is dropped
                let mut hasher = DefaultHasher::new();
                let tid = TypeId::of::<T>();
                tid.hash(&mut hasher);
                let v = hasher.finish();
                self.data
                    .insert(LocationGroup::<T>::new(self.capacity, self_ptr, v));
                self.data.get_mut::<LocationGroup<T>>().unwrap()
            }
        };
        let (generation, index): (usize, usize);
        match group.free_indexes.get_mut().pop() {
            Some(idx) => {
                let location = &mut group.locations[idx];
                generation = *location.generation.borrow();
                index = idx;
                location.entity = v;
            }
            None => {
                generation = 0;
                index = group.locations.len();
                group.locations.push(Location {
                    entity: v,
                    generation: RefCell::new(generation),
                })
            }
        };
        Address::<T> {
            generation,
            index,
            phantom: PhantomData,
            arena: self_ptr,
            ref_count: Rc::new(RefCell::new(1)),
        }
    }

    /// Mark the location of the address as free. This opens up that location and all remaining
    /// references will no longer be valid
    #[inline]
    pub fn free<T: 'static>(&self, address: &Address<T>) {
        // this checks prevents dereferencing a location after it has been freed
        let group = &self.data.get::<LocationGroup<T>>().unwrap();
        if self.freed_groups.contains(&group.type_id_hash) {
            return;
        }
        let location = &group.locations[address.index];
        if *location.generation.borrow() == address.generation {
            group.free_indexes.borrow_mut().push(address.index);
            *location.generation.borrow_mut() += 1;
        }
    }
}

impl Default for Arena {
    fn default() -> Self {
        Arena::new(DEFAULT_CAPACITY)
    }
}
