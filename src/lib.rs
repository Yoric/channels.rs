//! A variant of `std::sync::mpsc` that supports `map`-style operations.
//!
//! # The problem at hand
//!
//! Consider an event loop, defined as follows:
//!
//! ```ignore
//! let (tx, rx) = channel();
//! thread::spawn(move || {
//!   for event in rx {
//!     match event {
//!       UIEvent::KeyboardEvent(ev) => { ... },
//!       UIEvent::MouseEvent(ev) => { ... },
//!       ...
//!     }
//!   }
//! });
//! ```
//!
//! Now, imagine a system library that can watch for keyboard events, with the following signature:
//!
//! ```ignore
//! impl ThirdPartyLibrary {
//!   fn register_watch(&self, on_event: Sender<PrimitiveKeyboardEvent>) -> ...;
//! }
//! ```
//!
//! How can we interact with this library? Well, with `Sender`, the only way is to fire another
//! thread, as follows:
//!
//! ```ignore
//! let (tx2, rx2) = channel();
//! let tx = tx.clone(); // That's the tx for my event loop, see above.
//! thread::spawn(move || {
//!   for ev in rx {
//!     match tx.send(UIEvent::KeyboardEvent(ev) {
//!       Ok(_) => {},
//!       Err(_) => return, // Cleanup if nobody is listening anymore.
//!     }
//!   }
//! });
//!
//! third_party_library.register_watch(tx2);
//! ```
//!
//! Wouldn't it be nicer and more resource-efficient if we could write the following and have it
//! work without spawning a thread?
//!
//! ```ignore
//! third_party_library.register_watch(tx.map(|ev| UIEvent::KeyboardEvent(ev)));
//! ```
//!
//! Now, let's assume that the situation is slightly more complicated and that our system needs
//! to handle several keyboards. Now, we need to label each keyboard with a unique key.
//!
//! With `Sender`, the only solution is to fire *one thread per keyboard*, i.e.
//!
//! ```ignore
//! let key = ...;
//! let (tx3, rx3) = channel();
//! let tx = tx.clone(); // That's the tx for my event loop, see above.
//! thread::spawn(move || {
//!   for ev in rx {
//!     match tx.send(UIEvent::KeyboardEvent(key, ev) {
//!       Ok(_) => {},
//!       Err(_) => return, // Cleanup if nobody is listening anymore.
//!     }
//!   }
//! });
//!
//! third_party_library.register_watch(tx3);
//! ```
//!
//! Wouldn't it be nicer and more resource-efficient if we could write the following and have it
//! work without spawning a thread?
//!
//! ```ignore
//! let key = ...;
//! third_party_library.register_watch(tx.map(move |ev| UIEvent::KeyboardEvent(key, ev)));
//! ```
//!
//! This crate is designed to make the nicer and more resource-efficient strategy possible.
//!
//!
//! # Usage
//!
//! This crate is a drop-in replacement for `std::sync::mpsc`. The only difference is that
//! the type `ExtSender<T>`, which replaces `Sender<T>`, supports operations such as `map`,
//! `filter` and `filter_map` -- see the documentation of `TransformableSender` for all
//! details and examples.
use std::sync::mpsc::{ channel as std_channel, Sender };
pub use std::sync::mpsc::Receiver;

use std::sync::Arc;
use std::marker::PhantomData;
use std::ops::Deref;

/// Creates a new asynchronous channel, returning the sender/receiver halves.
/// All data sent on the sender will become available on the receiver, and no send will block
/// the calling thread (this channel has an "infinite buffer").
///
/// Receivers are identical to those of `std::sync::mpsc::Receiver`.
/// Senders have the same behavior as those of `std::sync::mpsc::Sender`, with the exception
/// of error values.
///
/// # Example
///
/// ```
/// use std::thread;
/// use transformable_channels::*;
///
/// let (tx, rx) = channel();
///
/// for i in 1 .. 5 {
///   tx.send(i).unwrap();
/// }
///
/// thread::spawn(move || {
///   for msg in rx {
///     println!("Received message {:?}", msg);
///   }
/// });
/// ```
pub fn channel<T>() -> (RawSender<T>, Receiver<T>) where T: Send + 'static {
    let (tx, rx) = std_channel();
    (RawSender { std_sender: tx }, rx)
}

/// The sending-half of this crate's asynchronous channel type. This half can only be owned by one
/// thread, but all instances can be cloned to let copies be shared with other threads.
///
/// All implementations of `ExtSender` provided in this crate implement `TransformableSender`,
/// which provides
pub trait ExtSender<T>: Send + 'static where T: Send + 'static {
    /// Attempts to send a value on this channel.
    ///
    /// A successful send occurs when it is determined that the other end of the channel has not
    /// hung up already. An unsuccessful send would be one where the corresponding receiver has
    /// already been deallocated. Note that a return value of `Err` means that the data will never
    /// be received, but a return value of `Ok` does not mean that the data will be received. It
    /// is possible for the corresponding receiver to hang up immediately after this function
    /// returns `Ok`.
    ///
    /// This method will never block the current thread.
    fn send(&self, t: T) -> Result<(), ()>;

    /// A low-level method used to define Clone(). Probably not useful outside of this crate. May
    /// disappear in future versions.
    fn id(&self) -> Box<ExtSender<T>>;
}

pub trait TransformableSender<V>: Send + 'static where V: Send + 'static, Self: ExtSender<V> + Clone {
    /// From an `ExtSender`, derive a new `ExtSender` with the same `Receiver`, but which transforms
    /// values prior to transmitting them.
    ///
    /// This allows, for instance, converting from one data type to another one, or labelling values
    /// with their origin.
    ///
    /// Note that the closure will often be shared between threads and must therefore implement `Sync`.
    ///
    /// # Safety and performance
    ///
    /// Argument `f` is executed during the call to `send`. Therefore, any panic in `f` will cause the
    /// sender thread to panic. If `f` is slow, the sender thread will be slowed down.
    ///
    /// # Example
    ///
    /// ```
    /// use std::thread;
    /// use transformable_channels::*;
    ///
    /// let (tx, rx) = channel();
    ///
    /// #[derive(Debug)]
    /// struct LabelledValue<T> {
    ///   origin: String,
    ///   value: T,
    /// }
    ///
    /// for i in 1 .. 5 {
    ///   // Let us label messages as they are sent.
    ///   let origin = format!("Thread: {}", i);
    ///   let tx = tx.map(move |msg| {
    ///      LabelledValue {
    ///        origin: origin.clone(),
    ///        value: msg,
    ///      }
    ///   });
    ///
    ///   thread::spawn(move || {
    ///     for i in 1 .. 5 {
    ///       tx.send(i).unwrap();
    ///     }
    ///   });
    /// }
    ///
    /// thread::spawn(move || {
    ///   for msg in rx {
    ///     // Will displayed 1 .. 5, labelled with their thread of origin.
    ///     println!("Received message {:?}", msg);
    ///   }
    /// });
    /// ```
    fn map<F, T>(&self, f: F) -> MappedSender<F, T, V> where
        F: Fn(T) -> V + Sync + Send + 'static,
        T: Send + 'static,
        V: Send + 'static
    {
        MappedSender {
            wrapped: self.id(),
            transformer: Arc::new(f),
            phantom: PhantomData
        }
    }

    /// From an `ExtSender`, derive a new `ExtSender` with the same `Receiver`, but which may decide
    /// to discard some values instead of transmitting them.
    ///
    /// This allows, for instance, discarding events that are not of interest.
    ///
    /// Note that the closure will often be shared between threads and must therefore implement `Sync`.
    ///
    /// # Safety and performance
    ///
    /// Argument `f` is executed during the call to `send`. Therefore, any panic in `f` will cause the
    /// sender thread to panic. If `f` is slow, the sender thread will be slowed down.
    ///
    /// # Example
    ///
    /// ```
    /// use std::thread;
    /// use transformable_channels::*;
    ///
    /// let (tx, rx) = channel();
    ///
    /// for i in 1 .. 5 {
    ///   // We are only interested in even messages
    ///   let tx = tx.filter(move |msg| {
    ///      msg % 2 == 0
    ///   });
    ///
    ///   thread::spawn(move || {
    ///     for i in 1 .. 5 {
    ///       tx.send(i).unwrap();
    ///     }
    ///   });
    /// }
    ///
    /// thread::spawn(move || {
    ///   for msg in rx {
    ///     println!("Received message {:?}", msg);
    ///   }
    /// });
    /// ```
    fn filter<F>(&self, f: F) -> FilteredSender<F, V> where
        F: Fn(&V) -> bool + Sync + Send + 'static,
    {
        FilteredSender {
            wrapped: self.id(),
            filter: Arc::new(f),
            phantom: PhantomData
        }
    }

    /// From an `ExtSender`, derive a new `ExtSender` with the same `Receiver`, but which may both
    /// transform values priori to transmitting them, or to discard them entirely.
    ///
    /// This combines the features of `map` and `filter`.
    ///
    /// Note that the closure will often be shared between threads and must therefore implement `Sync`.
    ///
    /// # Safety and performance
    ///
    /// Argument `f` is executed during the call to `send`. Therefore, any panic in `f` will cause the
    /// sender thread to panic. If `f` is slow, the sender thread will be slowed down.
    ///
    /// # Example
    ///
    /// ```
    /// use std::thread;
    /// use transformable_channels::*;
    ///
    /// let (tx, rx) = channel();
    ///
    /// for i in 1 .. 5 {
    ///   // We are only interested in even messages
    ///   let tx = tx.filter_map(move |msg| {
    ///      if msg % 2 == 0 {
    ///        Some("This was an even message")
    ///      } else {
    ///        None
    ///      }
    ///   });
    ///
    ///   thread::spawn(move || {
    ///     for i in 1 .. 5 {
    ///       tx.send(i).unwrap();
    ///     }
    ///   });
    /// }
    ///
    /// thread::spawn(move || {
    ///   for msg in rx {
    ///     println!("Received message {:?}", msg);
    ///   }
    /// });
    /// ```
    fn filter_map<F, T, U: ?Sized>(&self, f: F) -> FilterMappedSender<F, T, V> where
        F: Fn(T) -> Option<V> + Sync + Send + 'static,
        T: Send + 'static,
    {
        FilterMappedSender {
            wrapped: self.id(),
            transformer: Arc::new(f),
            phantom: PhantomData
        }
    }
}

/// A trait object for an ExtSender.
impl<T> ExtSender<T> for Box<ExtSender<T>> where T: Send + 'static {
    fn send(&self, t: T) -> Result<(), ()> {
        self.deref().send(t)
    }
    fn id(&self) -> Box<ExtSender<T>> {
        self.deref().id()
    }
}
impl<T> Clone for Box<ExtSender<T>> where T: Send + 'static {
  fn clone(&self) -> Self {
      self.id()
  }
}
impl<T> TransformableSender<T> for Box<ExtSender<T>> where T: Send + 'static { }

/// An implementation of `ExtSender` directly on top of `std::sync::mpsc::Sender` and with
/// the same performance.
pub struct RawSender<T> where T: Send + 'static {
    std_sender: Sender<T>
}
impl<T> ExtSender<T> for RawSender<T> where T: Send + 'static {
    /// Let the wrapped `Sender` send the value. Discard error values.
    fn send(&self, t: T) -> Result<(), ()> {
        match (self.std_sender).send(t) {
            Ok(_) => Ok(()),
            Err(_) => Err(())
        }
    }

    /// Clone the wrapped `Sender` and wrap the result.
    fn id(&self) -> Box<ExtSender<T>> {
        Box::new(RawSender {
            std_sender: self.std_sender.clone()
        })
    }
}
impl<T> Clone for RawSender<T> where T: Send + 'static {
    fn clone(&self) -> Self {
        RawSender {
            std_sender: self.std_sender.clone()
        }
    }
}
impl<T> TransformableSender<T> for RawSender<T> where T: Send + 'static { }

/// An `ExtSender` obtained from a call to method `filter_map`.
pub struct FilterMappedSender<F, T, V> where
    F: Fn(T) -> Option<V> + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static,
{
    wrapped: Box<ExtSender<V>>,
    transformer: Arc<F>,
    phantom: PhantomData<(T, V)>
}
impl<F, T, V> ExtSender<T> for FilterMappedSender<F, T, V> where
    F: Fn(T) -> Option<V> + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{
    fn send(&self, t: T) -> Result<(), ()> {
        match (self.transformer)(t) {
            None => Ok(()),
            Some(t2) => (self.wrapped).send(t2)
        }
    }
    fn id(&self) -> Box<ExtSender<T>> {
        Box::new(FilterMappedSender {
            wrapped: self.wrapped.id(),
            transformer: self.transformer.clone(),
            phantom: PhantomData::<(T, V)>
        })
    }
}
impl<F, T, V> Clone for FilterMappedSender<F, T, V>  where
    F: Fn(T) -> Option<V> + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{
    fn clone(&self) -> Self {
        FilterMappedSender {
            wrapped: self.wrapped.id(),
            transformer: self.transformer.clone(),
            phantom: PhantomData
        }
    }
}
impl<F, T, V> TransformableSender<T> for FilterMappedSender<F, T, V>  where
    F: Fn(T) -> Option<V> + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{}

    /// An `ExtSender` obtained from a call to method `filter`.
pub struct FilteredSender<F, T> where
    F: Fn(&T) -> bool + Sync + Send + 'static,
    T: Send + 'static,
{
    wrapped: Box<ExtSender<T>>,
    filter: Arc<F>,
    phantom: PhantomData<T>
}
impl<F, T> ExtSender<T> for FilteredSender<F, T> where
    F: Fn(&T) -> bool + Sync + Send + 'static,
    T: Send + 'static,
{
    fn send(&self, t: T) -> Result<(), ()> {
        if (self.filter)(&t) {
            (self.wrapped).send(t)
        } else {
            Ok(())
        }
    }
    fn id(&self) -> Box<ExtSender<T>> {
        Box::new(FilteredSender {
            wrapped: self.wrapped.id(),
            filter: self.filter.clone(),
            phantom: PhantomData
        })
    }
}
impl<F, T> Clone for FilteredSender<F, T> where
    F: Fn(&T) -> bool + Sync + Send + 'static,
    T: Send + 'static,
{
    fn clone(&self) -> Self {
        FilteredSender {
            wrapped: self.wrapped.clone(),
            filter: self.filter.clone(),
            phantom: PhantomData
        }
    }
}
impl<F, T> TransformableSender<T> for FilteredSender<F, T> where
    F: Fn(&T) -> bool + Sync + Send + 'static,
    T: Send + 'static,
{}

pub struct MappedSender<F, T, V> where
    F: Fn(T) -> V + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static,
{
    wrapped: Box<ExtSender<V>>,
    transformer: Arc<F>,
    phantom: PhantomData<(T, V)>
}
impl<F, T, V> ExtSender<T> for MappedSender<F, T, V> where
    F: Fn(T) -> V + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{
    fn send(&self, t: T) -> Result<(), ()> {
        (self.wrapped).send((self.transformer)(t))
    }
    fn id(&self) -> Box<ExtSender<T>> {
        Box::new(MappedSender {
            wrapped: self.wrapped.id(),
            transformer: self.transformer.clone(),
            phantom: PhantomData::<(T, V)>
        })
    }
}
impl<F, T, V> Clone for MappedSender<F, T, V> where
    F: Fn(T) -> V + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{
    fn clone(&self) -> Self {
        MappedSender {
            wrapped: self.wrapped.clone(),
            transformer: self.transformer.clone(),
            phantom: PhantomData
        }
    }
}
impl<F, T, V> TransformableSender<T> for MappedSender<F, T, V> where
    F: Fn(T) -> V + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{}

#[test]
fn test() {
    // Compilation test: ensure that we can chain `map` several times.
    let (tx, rx) = channel();
    tx.map(|x| x + 1).map(|x| x - 1).send(0).unwrap();;
    assert_eq!(rx.recv().unwrap(), 0);

/*
    let (tx, rx) = channel::<i32>();
    let tx2 = tx.filter(|x| *x == 0).map(|x| x + 1);
    let tx3 = tx.filter_map(|x: i32| if x == 0 { Some (x + 1) } else { None });
    for i in 1 .. 10 {
        tx2.send(i);
        tx3.send(i);
    }
    drop(tx2);
    drop(tx3);
    drop(tx);
    for i in 1 .. 10 {
        assert_eq!(rx.recv().unwrap(), rx.recv().unwrap());
    }
    for msg in rx {
        panic!("Well, there shouldn't be any message left");
    }
*/
}