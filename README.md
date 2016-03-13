[![Build Status](https://api.travis-ci.org/Yoric/channels.rs.svg?branch=master)](https://travis-ci.org/Yoric/channels.rs)

[Documentation](http://yoric.github.io/channels.rs/doc/transformable_channels/)

# The problem at hand

Consider an event loop, defined as follows:

```rust
let (tx, rx) = channel();
thread::spawn(move || {
  for event in rx {
    match event {
      UIEvent::KeyboardEvent(ev) => { ... },
      UIEvent::MouseEvent(ev) => { ... },
      ...
    }
  }
});
```

Now, imagine a system library that can watch for keyboard events, with the following signature:

```rust
impl ThirdPartyLibrary {
  fn register_watch(&self, on_event: Sender<PrimitiveKeyboardEvent>) -> ...;
}
```

How can we interact with this library? Well, with `Sender`, the only way is to fire another
thread, as follows:

```rust
let (tx2, rx2) = channel();
let tx = tx.clone(); // That's the tx for my event loop, see above.
thread::spawn(move || {
  for ev in rx {
    match tx.send(UIEvent::KeyboardEvent(ev) {
      Ok(_) => {},
      Err(_) => return, // Cleanup if nobody is listening anymore.
    }
  }
});

third_party_library.register_watch(tx2);
```

Wouldn't it be nicer and more resource-efficient if we could write the following and have it
work without spawning a thread?

```rust
third_party_library.register_watch(tx.map(|ev| UIEvent::KeyboardEvent(ev)));
```

Now, let's assume that the situation is slightly more complicated and that our system needs
to handle several keyboards. Now, we need to label each keyboard with a unique key.

With `Sender`, the only solution is to fire *one thread per keyboard*, i.e.

```rust
let key = ...;
let (tx3, rx3) = channel();
let tx = tx.clone(); // That's the tx for my event loop, see above.
thread::spawn(move || {
  for ev in rx {
    match tx.send(UIEvent::KeyboardEvent(key, ev) {
      Ok(_) => {},
      Err(_) => return, // Cleanup if nobody is listening anymore.
    }
  }
});

third_party_library.register_watch(tx3);
```

Wouldn't it be nicer and more resource-efficient if we could write the following and have it
work without spawning a thread?

```rust
let key = ...;
third_party_library.register_watch(tx.map(move |ev| UIEvent::KeyboardEvent(key, ev)));
```

This crate is designed to make the nicer and more resource-efficient strategy possible.

# Example

```rust
 use transformable_channels::mpsc::*;
 let (tx, rx) = channel();

 // You can use `tx` and `rx` exactly as usual.

 tx.send("Hello".to_owned());
 assert_eq!(rx.recv().unwrap(), "Hello".to_owned());

 // In addition, `tx` can be cheaply transformed through the use of `map`

 let tx_number = tx.map(|num: i32| format!("Number {}", num));
 tx_number.send(42);
 assert_eq!(rx.recv().unwrap(), "Number 42".to_owned());


 // We also have `filter`.

 let tx_odd = tx_number.filter(|num| num % 2 == 0);
 tx_odd.send(41); // This one will be skipped.
 tx_odd.send(42);
 assert_eq!(rx.recv().unwrap(), "Number 42".to_owned());


 // Instead of `map` + `filter`, `filter_map` works nicely, too.

 let tx_odd_2 = tx.filter_map(|num: i32| {
   if num % 2 == 0 {
     Some(format!("Number {}", num))
   } else {
     None
   }
 });
 tx_odd_2.send(41); // This one will be skipped.
 tx_odd_2.send(42);
 assert_eq!(rx.recv().unwrap(), "Number 42".to_owned());

// ...
```
