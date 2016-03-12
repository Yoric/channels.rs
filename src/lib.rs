//! A variant of `std::sync::mpsc` that supports `map`-style operations
use std::sync::mpsc::{channel as std_channel, Receiver, Sender };
use std::sync::Arc;
use std::marker::PhantomData;
use std::ops::Deref;

pub fn channel<T>() -> (RawSender<T>, Receiver<T>) where T: Send + 'static {
    let (tx, rx) = std_channel();
    (RawSender { sender: tx}, rx)
}

pub trait ExtSender<T>: Send + 'static where T: Send + 'static {
    fn send(&self, t: T) -> Result<(), ()>;
    fn clone(&self) -> Box<ExtSender<T>>
    {
        unimplemented!()
    }
/*
    fn map<F, A>(&self, f: F) -> MappedSender<F, A, ExtSender<T>, T> where
        F: Fn(A) -> T + Send + Sync,
        A: Send + 'static,
    {
        map(unimplemented!(), f)
    }
*/
}

impl<T, U> ExtSender<T> for Box<U> where U: ExtSender<T>, T: Send + 'static {
    fn send(&self, t: T) -> Result<(), ()> {
        self.deref().send(t)
    }
}

pub struct RawSender<T> where T: Send + 'static {
    sender: Sender<T>
}
impl<T> ExtSender<T> for RawSender<T> where T: Send + 'static {
    fn send(&self, t: T) -> Result<(), ()> {
        match (self.sender).send(t) {
            Ok(_) => Ok(()),
            Err(_) => Err(())
        }
    }
}
impl<T> Clone for RawSender<T> where T: Send + 'static {
    fn clone(&self) -> Self {
        RawSender {
            sender: self.sender.clone()
        }
    }
}

pub struct MappedSender<F, T, U: ?Sized, V> where
    U: ExtSender<V>,
    F: Fn(T) -> V + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static,
{
    wrapped: Box<U>,
    transformer: Arc<F>,
    phantom: PhantomData<(T, U, V)>
}
impl<F, T, U: ?Sized, V> ExtSender<T> for MappedSender<F, T, U, V> where
    U: ExtSender<V>,
    F: Fn(T) -> V + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{
    fn send(&self, t: T) -> Result<(), ()> {
        (self.wrapped).send((self.transformer)(t))
    }
}
impl<F, T, U: ?Sized, V> Clone for MappedSender<F, T, U, V> where
    U: ExtSender<V>,
    F: Fn(T) -> V + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{
    fn clone(&self) -> Self {
        MappedSender {
            wrapped: self.wrapped.clone(),
            transformer: self.transformer.clone(),
            phantom: PhantomData::<(T, U, V)>
        }
    }
}

pub fn map<F, T, U: ?Sized, V>(sender: &U, f: F) -> MappedSender<F, T, U, V> where
    U: ExtSender<V>,
    F: Fn(T) -> V + Sync + Send + 'static,
    T: Send + 'static,
    V: Send + 'static
{
    MappedSender {
        wrapped: /*Box::new(sender.clone())*/unimplemented!(),
        transformer: Arc::new(f),
        phantom: PhantomData
    }
}

#[test]
fn test() {
    use std::thread;
    use std::time::Duration;

    let (tx, rx) = channel();
    thread::spawn(move || {
        for msg in rx {
            println!("msg {:?}", msg);
        }
    });
    for i in 1..3 {
        let i = i.clone();
        let tx = tx.map(move |msg| (i, msg));
        thread::spawn(move || {
            for i in 1..5 {
                let _ = tx.send(format!("{}", i));
            }
        });
    }

    thread::sleep(Duration::new(5, 1));
}