//! A variant of `std::sync::mpsc` that supports `map`-style operations.
//!
//!
//! This crate is near a drop-in replacement for `std::sync::mpsc`. The only difference is that
//! the type `ExtSender<T>`, which replaces `Sender<T>`, supports operations such as `map`,
//! `filter` and `filter_map` -- see the documentation of `TransformableSender` for all
//! details and examples.
//!
//! The type `Receiver<T>` is reexported from std::sync::mpsc::Receiver.
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
///   for (msg, i) in rx.iter().zip(1..5) {
///     assert_eq!(msg, i);
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
    fn internal_clone(&self) -> Box<ExtSender<T>>;
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
    /// use transformable_channels::*;
    ///
    /// let (tx, rx) = channel();
    ///
    /// #[derive(Debug)]
    /// struct LabelledValue<T> {
    ///   origin: usize,
    ///   value: T,
    /// }
    ///
    /// for i in 1 .. 5 {
    ///   // Let us label messages as they are sent.
    ///   let tx = tx.map(move |msg| {
    ///      LabelledValue {
    ///        origin: i,
    ///        value: msg,
    ///      }
    ///   });
    ///
    ///   tx.send(i).unwrap();
    /// }
    ///
    /// for _ in 1 .. 5 {
    ///   let msg = rx.recv().unwrap();
    ///   assert_eq!(msg.origin, msg.value);
    /// }
    /// ```
    fn map<F, T>(&self, f: F) -> MappedSender<F, T, V, Self> where
        Self: Sized,
        F: Fn(T) -> V + Sync + Send + 'static,
        T: Send + 'static,
        V: Send + 'static
    {
        MappedSender {
            wrapped: self.clone(),
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
    /// use transformable_channels::*;
    ///
    /// let (tx, rx) = channel();
    ///
    /// // We are only interested in even messages
    /// let tx = tx.filter(move |msg| {
    ///    msg % 2 == 0
    /// });
    ///
    /// for i in 1 .. 10 {
    ///   tx.send(i).unwrap();
    /// }
    ///
    /// for _ in 1 .. 5 {
    ///   assert!(rx.recv().unwrap() % 2 == 0);
    /// }
    /// ```
    fn filter<F>(&self, f: F) -> FilteredSender<F, V, Self> where
        Self: Sized,
        F: Fn(&V) -> bool + Sync + Send + 'static,
    {
        FilteredSender {
            wrapped: self.clone(),
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
    /// use transformable_channels::*;
    ///
    /// let (tx, rx) = channel();
    ///
    /// // We are only interested in even messages
    /// let tx = tx.filter_map(move |msg| {
    ///    if msg % 2 == 0 {
    ///      Some( msg + 1 )
    ///    } else {
    ///      None
    ///    }
    /// });
    ///
    /// for i in 1 .. 10 {
    ///   tx.send(i).unwrap();
    /// }
    ///
    /// for i in 1 .. 5 {
    ///   assert_eq!(rx.recv().unwrap(), i * 2 + 1);
    /// }
    /// ```
    fn filter_map<F, T>(&self, f: F) -> FilterMappedSender<F, T, V, Self> where
        Self: Sized,
        F: Fn(T) -> Option<V> + Sync + Send + 'static,
        T: Send + 'static,
    {
        FilterMappedSender {
            wrapped: self.clone(),
            transformer: Arc::new(f),
            phantom: PhantomData
        }
    }

    /// From two `ExtSender`s, derive a new `ExtSender` which sends to both `ExtSender`s.
    ///
    /// # Example
    ///
    /// ```
    /// use transformable_channels::*;
    ///
    /// let (tx_1, rx_1) = channel();
    /// let (tx_2, rx_2) = channel();
    ///
    /// let tx = tx_1.tee(&tx_2);
    ///
    /// tx.send(("one".to_string(), 1)).unwrap();
    /// assert_eq!(rx_1.recv().unwrap(), "one".to_string());
    /// assert_eq!(rx_2.recv().unwrap(), 1);
    /// ```
    fn tee<S, W>(&self, other: &S) -> TeedSender<Self, S, V, W> where
        Self: Sized,
        S: ExtSender<W> + TransformableSender<W>,
        W: Send + 'static,
    {
        TeedSender {
            left: self.clone(),
            right: other.clone(),
            phantom: PhantomData
        }
    }

}

/// A trait object for an ExtSender.
impl<T> ExtSender<T> for Box<ExtSender<T>> where T: Send + 'static {
    fn send(&self, t: T) -> Result<(), ()> {
        self.deref().send(t)
    }
    fn internal_clone(&self) -> Box<ExtSender<T>> {
        self.deref().internal_clone()
    }
}
impl<T> Clone for Box<ExtSender<T>> where T: Send + 'static {
  fn clone(&self) -> Self {
      self.internal_clone()
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
    fn internal_clone(&self) -> Box<ExtSender<T>> {
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
pub struct FilterMappedSender<F, T, V, W> where
    W: ExtSender<V> + TransformableSender<V> + Sized,
    F: Fn(T) -> Option<V> + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static,
{
    wrapped: W,
    transformer: Arc<F>,
    phantom: PhantomData<(T, V)>
}
impl<F, T, V, W> ExtSender<T> for FilterMappedSender<F, T, V, W> where
    W: ExtSender<V> + TransformableSender<V> + Sized,
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
    fn internal_clone(&self) -> Box<ExtSender<T>> {
        Box::new(FilterMappedSender {
            wrapped: self.wrapped.clone(),
            transformer: self.transformer.clone(),
            phantom: PhantomData::<(T, V)>
        })
    }
}
impl<F, T, V, W> Clone for FilterMappedSender<F, T, V, W>  where
    W: ExtSender<V> + TransformableSender<V> + Sized,
    F: Fn(T) -> Option<V> + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{
    fn clone(&self) -> Self {
        FilterMappedSender {
            wrapped: self.wrapped.clone(),
            transformer: self.transformer.clone(),
            phantom: PhantomData
        }
    }
}
impl<F, T, V, W> TransformableSender<T> for FilterMappedSender<F, T, V, W>  where
    W: ExtSender<V> + TransformableSender<V> + Sized,
    F: Fn(T) -> Option<V> + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{}

/// An `ExtSender` obtained from a call to method `filter`.
pub struct FilteredSender<F, T, W> where
    F: Fn(&T) -> bool + Sync + Send + 'static,
    T: Send + 'static,
    W: ExtSender<T> + TransformableSender<T> + Sized,
{
    wrapped: W,
    filter: Arc<F>,
    phantom: PhantomData<T>
}
impl<F, T, W> ExtSender<T> for FilteredSender<F, T, W> where
    F: Fn(&T) -> bool + Sync + Send + 'static,
    T: Send + 'static,
    W: ExtSender<T> + TransformableSender<T> + Sized,
{
    fn send(&self, t: T) -> Result<(), ()> {
        if (self.filter)(&t) {
            (self.wrapped).send(t)
        } else {
            Ok(())
        }
    }
    fn internal_clone(&self) -> Box<ExtSender<T>> {
        Box::new(FilteredSender {
            wrapped: self.wrapped.clone(),
            filter: self.filter.clone(),
            phantom: PhantomData
        })
    }
}
impl<F, T, W> Clone for FilteredSender<F, T, W> where
    F: Fn(&T) -> bool + Sync + Send + 'static,
    T: Send + 'static,
    W: ExtSender<T> + TransformableSender<T> + Sized,
{
    fn clone(&self) -> Self {
        FilteredSender {
            wrapped: self.wrapped.clone(),
            filter: self.filter.clone(),
            phantom: PhantomData
        }
    }
}
impl<F, T, W> TransformableSender<T> for FilteredSender<F, T, W> where
    F: Fn(&T) -> bool + Sync + Send + 'static,
    T: Send + 'static,
    W: ExtSender<T> + TransformableSender<T> + Sized,
{}

/// An `ExtSender` obtained from a call to method `map`.
pub struct MappedSender<F, T, V, W> where
    W: ExtSender<V> + TransformableSender<V> + Sized,
    F: Fn(T) -> V + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static,
{
    wrapped: W,
    transformer: Arc<F>,
    phantom: PhantomData<(T, V)>
}
impl<F, T, V, W> ExtSender<T> for MappedSender<F, T, V, W> where
    W: ExtSender<V> + TransformableSender<V> + Sized,
    F: Fn(T) -> V + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{
    fn send(&self, t: T) -> Result<(), ()> {
        (self.wrapped).send((self.transformer)(t))
    }
    fn internal_clone(&self) -> Box<ExtSender<T>> {
        Box::new(MappedSender {
            wrapped: self.wrapped.clone(),
            transformer: self.transformer.clone(),
            phantom: PhantomData::<(T, V)>
        })
    }
}
impl<F, T, V, W> Clone for MappedSender<F, T, V, W> where
    W: ExtSender<V> + TransformableSender<V> + Sized,
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
impl<F, T, V, W> TransformableSender<T> for MappedSender<F, T, V, W> where
    W: ExtSender<V> + TransformableSender<V> + Sized,
    F: Fn(T) -> V + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{}

/// An `ExtSender` obtained from a call to method `tee`.
pub struct TeedSender<S1, S2, V1, V2> where
    S1: ExtSender<V1> + TransformableSender<V1> + Sized,
    S2: ExtSender<V2> + TransformableSender<V2> + Sized,
    V1: Send + 'static,
    V2: Send + 'static,
{
    left: S1,
    right: S2,
    phantom: PhantomData<(V1, V2)>
}
impl<S1, S2, V1, V2> ExtSender<(V1, V2)> for TeedSender<S1, S2, V1, V2>
where
    S1: ExtSender<V1> + TransformableSender<V1> + Sized,
    S2: ExtSender<V2> + TransformableSender<V2> + Sized,
    V1: Send + 'static,
    V2: Send + 'static,
{
    fn send(&self, (left, right): (V1, V2)) -> Result<(), ()> {
        match (self.left.send(left), self.right.send(right)) {
            (Err(()), Err(())) => Err(()), // By spec, return `Err()` only if we are sure that
                // the message didn't reach is destination.
            _ => Ok(()) // Otherwise, return `Ok()`
        }
    }
    fn internal_clone(&self) -> Box<ExtSender<(V1, V2)>> {
        Box::new(TeedSender {
            left: self.left.clone(),
            right: self.right.clone(),
            phantom: PhantomData
        })
    }
}
impl<S1, S2, V1, V2> Clone for TeedSender<S1, S2, V1, V2>
where
    S1: ExtSender<V1> + TransformableSender<V1> + Sized,
    S2: ExtSender<V2> + TransformableSender<V2> + Sized,
    V1: Send + 'static,
    V2: Send + 'static,
{
    fn clone(&self) -> Self {
        TeedSender {
            left: self.left.clone(),
            right: self.right.clone(),
            phantom: PhantomData
        }
    }
}

#[test]
fn test_chain_map() {
    println!("* Making sure that chain composition of .map() takes place in the right order");
    let (tx, rx) = channel();
    let tx = tx.map(|x| x + 1).map(|x| x * 2);
    tx.send(0).unwrap();
    assert_eq!(rx.recv().unwrap(), 1);
}


#[test]
fn test_filter_map() {
    let (tx, rx) = channel();
    let tx2 = (tx.map(|x| { x + 1 })).filter(|x| x % 2 == 0);
    let tx3 = tx.filter_map(|x| if x%2 == 0 { Some (x + 1) } else { None });

    for i in 1 .. 10 {
        tx2.send(i).unwrap();
        tx3.send(i).unwrap();
        if i % 2 == 0 {
            let got_2 = rx.recv().unwrap();
            let got_3 = rx.recv().unwrap();
            assert_eq!(got_2, got_3);
        }
    }
}