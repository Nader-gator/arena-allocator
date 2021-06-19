/*!
# This file contains a demo and a performance test of the arena allocator. For the interesting parts,
look into the `src/allocator/arena.rs` file
 */

use arena_allocator::{Address, Arena};
use std::cell::RefCell;
use std::rc::Rc;

/// This is a small demo of the arena in action. It shows how pointers can be easily copied around,
/// referenced and freed, without any null pointers, undefined behavior, and without losing any
/// of rusts type safety and guarantees.
#[allow(dead_code, unused_variables)]
fn demo() {
    #[derive(Debug)]
    struct Health {
        value: i8,
    }
    #[derive(Debug)]
    struct Human {
        name: String,
        health: Address<Health>,
        enemy: Address<Monster>,
        enemy_stooges: Vec<Address<Monster>>,
    }
    #[derive(Debug)]
    struct Monster {
        name: String,
        health: Address<Health>,
        target: Option<Address<Human>>,
        friend: Option<Address<Monster>>,
    }
    let mut arena = Arena::default();
    let main_enemy_health = arena.allocate(Health { value: 50 });
    let human_health = arena.allocate(Health { value: 100 });
    let main_enemy = arena.allocate(Monster {
        name: format!("Borrow checker"),
        health: main_enemy_health.copy(),
        target: None,
        friend: None,
    });
    let human = arena.allocate(Human {
        name: format!("Nader"),
        health: human_health.copy(),
        enemy: main_enemy.copy(),
        enemy_stooges: vec![],
    });
    main_enemy.get_mut().unwrap().target = Some(human.copy());
    // create 5 stooges and make human aware of them
    for i in 0..5 {
        let human_ptr = human.copy();
        let stooge_health = arena.allocate(Health { value: 10 });
        let stooge = arena.allocate(Monster {
            name: format!("stooge #{}", i + 1),
            health: stooge_health.copy(),
            target: Some(human_ptr.copy()),
            friend: None,
        });
        human_ptr
            .get_mut()
            .unwrap()
            .enemy_stooges
            .push(stooge.copy());
        // the last stooge is the main enemy's friend
        if i == 4 {
            main_enemy.get_mut().unwrap().friend = Some(stooge.copy())
        }
    }
    {
        // Scope 1, human looks at all the stooges, and kills the first one he sees which happens
        // to be the one the main_enemy holds a reference to as its friend
        //
        //
        let human_ptr = human.copy();
        // human wants count of all enemies
        match human_ptr.get_mut() {
            Some(h) => {
                // make sure all references are valid
                println!("Human sees {} stooges", h.enemy_stooges.len());
                // human kills the last valid stooge
                loop {
                    match h.enemy_stooges.pop() {
                        Some(stooge_ptr) => match stooge_ptr.copy().get() {
                            Some(stooge) => {
                                println!("attacking a stooge with name {}", stooge.name);
                                stooge_ptr.remove();
                                println!("got one, now there's {} left", h.enemy_stooges.len());
                                break;
                            }
                            None => println!("this stooge was killed by someone else"),
                        },
                        None => break,
                    }
                    println!("got one, now there's {} left", h.enemy_stooges.len());
                }
            }
            None => println!("Human died"),
        }
    }
    {
        // Meanwhile, in a completely different scope, where pointers are unaware that they are
        // referencing invalid data
        //
        //
        let monster_ptr = main_enemy.copy();

        // monster checks on friend, which was the one that was killed by human in the other scope
        match monster_ptr.get() {
            Some(monster) => {
                // Rust's type system forces the programmer to handle all cases and eliminates
                // undefined behavior
                match monster.friend.as_ref().unwrap().get() {
                    Some(friend) => println!("Monster says hello to {}", friend.name),
                    None => {
                        println!("monster sees friend is dead is now sad");
                        // monster hits human and reduces its health
                        if let Some(human) = monster.target.as_ref().unwrap().get() {
                            if let Some(health) = human.health.get_mut() {
                                health.value -= 5;
                            };
                        };
                    }
                };
            }
            None => println!("Monster was killed by someone else"),
        }
        let human_ptr = human.copy();
        if let Some(human) = human_ptr.get() {
            if let Some(health) = human.health.get() {
                println!("human health now is {}", health.value)
            }
        };
    }
    println!("Demo done")
}

/// This function shows the performance difference between arena allocation and heap allocation.
/// Adding items is not measured here but freeing them is, since freeing is where arena's really
/// shine. Specifically, deeply self referential data types, where deallocating an entity could
/// mean hundreds of drop() calls. In arenas, this as as simple as bumping an integer.
/// This is done by allocating a large number of structs, and starting a timer right before they go
/// out of scope and get freed.
///
/// If you're running this, I recommend compiling with optimizations to run this, as it runs slow
/// without it (command is `cargo build --release && ./target/release/arena-allocator `)
#[allow(dead_code, unused_variables)]
fn performance() {
    use std::time::Instant;

    #[derive(Default)]
    struct BigDataArena {
        data1: i128,
        val: Option<Address<BigDataArena>>,
    }

    #[derive(Default)]
    struct BigDataBox {
        data1: i128,
        val: Option<Rc<RefCell<BigDataBox>>>,
    }
    static ITEMS_COUNT: usize = 10_000_000;

    // Arena allocator ============
    let now;
    let mut arena = Arena::new(ITEMS_COUNT);
    {
        let mut curr: Option<Address<BigDataArena>> = None;
        let mut addresses = Vec::<Address<BigDataArena>>::with_capacity(ITEMS_COUNT);
        for _ in 0..ITEMS_COUNT {
            let new_obj = arena.allocate(BigDataArena::default());
            addresses.push(new_obj.copy());
            let new_obj_ptr = new_obj.copy();
            match curr {
                Some(ref add) => {
                    add.get_mut().unwrap().val = Some(new_obj);
                    curr = Some(new_obj_ptr)
                }
                None => curr = Some(new_obj),
            }
        }
        now = Instant::now();
    }
    let later = Instant::now();
    let allocator_elapsed = later - now;
    // ============================

    // Heap allocation ============
    let now;
    {
        let root = BigDataBox {
            data1: 0,
            val: None,
        };
        let mut curr = Rc::new(RefCell::new(root));
        let mut addresses = Vec::<Rc<RefCell<BigDataBox>>>::with_capacity(ITEMS_COUNT);
        for _ in 1..ITEMS_COUNT {
            let new_v = Rc::new(RefCell::new(BigDataBox {
                data1: 0,
                val: None,
            }));
            curr.borrow_mut().val = Some(Rc::clone(&new_v));
            addresses.push(Rc::clone(&new_v));
            curr = new_v;
        }
        now = Instant::now();
    }
    let later = Instant::now();
    let box_elapsed = later - now;
    // ============================

    println!("===============Perf results===============");
    println!("it took the allocator {:?}", allocator_elapsed);
    println!("it took the box {:?}", box_elapsed);
    println!("==========================================");
}

fn main() {
    performance();
    demo();
}
