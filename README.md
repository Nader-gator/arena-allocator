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

```
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

```
Humans: [human1, human2]
    | pointer_to_human2<index=1, gen=0> == human2

// human2 freed, human3 added

Humans: [human1, human3]
    | pointer_to_human2<index=1, gen=0> == human3
```

`pointer_to_human2` expects `human2`, but actually gets `human3`. Now with generations:

```
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
