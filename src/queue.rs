use anyhow::{anyhow, Result};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const BUFFER_SIZE: u64 = 32;

pub struct Queue<T: Default + Copy + Debug, P: Producer, C: Consumer, S: Sender<T>, R: Receiver<T>>
{
    consumer: C,
    producer: P,
    queue: *mut T,
    phantom_sender: PhantomData<S>,
    phantom_receiver: PhantomData<R>,
}

impl<
        T: Default + Copy + Debug + 'static,
        P: Producer + 'static,
        C: Consumer + 'static,
        S: Sender<T> + 'static,
        R: Receiver<T> + 'static,
    > Queue<T, P, C, S, R>
{
    fn new() -> Self {
        Self {
            consumer: C::new(),
            producer: P::new(),
            queue: vec![T::default(); BUFFER_SIZE as usize].leak().as_mut_ptr(),
            phantom_sender: PhantomData::default(),
            phantom_receiver: PhantomData::default(),
        }
    }

    pub fn channel() -> (S, R) {
        // Todo: make Queue drop with the receivers using an Arc
        let queue = Arc::new(Self::new());
        let queue_pop = queue.clone();

        let push = Box::new(move |e: T| queue.push(e));
        let pop = Box::new(move || queue_pop.pop());

        (S::new(push), R::new(pop))
    }

    fn pop(&self) -> Result<T> {
        let tail = self.consumer.tail();
        let ready = self.producer.ready();

        if ready <= tail {
            return Err(anyhow!("Empty"));
        }

        self.consumer.pop(tail, self.queue)
    }

    fn push(&self, i: T) -> Result<()> {
        // Todo: Handle wraparound
        let head = self.producer.head();
        let tail = self.consumer.tail();

        if tail + BUFFER_SIZE <= head {
            return Err(anyhow!("Queue full"));
        }

        self.producer.push(head, self.queue, i)
    }
}

pub trait Producer {
    fn new() -> Self;
    fn head(&self) -> u64;
    fn ready(&self) -> u64;
    fn push<T: Debug>(&self, head: u64, queue: *mut T, e: T) -> Result<()>;
}

pub trait Consumer {
    fn new() -> Self;
    fn tail(&self) -> u64;
    fn pop<T: Copy + Debug>(&self, tail: u64, queue: *mut T) -> Result<T>;
}

pub struct SingleProducer {
    write_progress: AtomicU64,
}

impl Producer for SingleProducer {
    fn new() -> SingleProducer {
        SingleProducer {
            write_progress: AtomicU64::new(0),
        }
    }
    fn head(&self) -> u64 {
        self.write_progress.load(Ordering::Relaxed)
    }
    fn ready(&self) -> u64 {
        self.head()
    }
    fn push<T: Debug>(&self, head: u64, queue: *mut T, e: T) -> Result<()> {
        unsafe {
            let p = queue.offset((head % BUFFER_SIZE) as isize);
            *p = e;
            std::sync::atomic::fence(Ordering::SeqCst);
            self.write_progress.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }
}

pub struct MultiProducer {
    write_progress: AtomicU64,
    commit_progress: AtomicU64,
}

impl Producer for MultiProducer {
    fn new() -> MultiProducer {
        MultiProducer {
            write_progress: AtomicU64::new(0),
            commit_progress: AtomicU64::new(0),
        }
    }
    fn head(&self) -> u64 {
        self.commit_progress.load(Ordering::Relaxed)
    }
    fn ready(&self) -> u64 {
        self.write_progress.load(Ordering::Relaxed)
    }
    fn push<T>(&self, head: u64, queue: *mut T, e: T) -> Result<()> {
        unsafe {
            if let Ok(_) = self.commit_progress.compare_exchange(
                head,
                head + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                let p = queue.offset((head % BUFFER_SIZE) as isize);
                *p = e;
                std::sync::atomic::fence(Ordering::SeqCst);
                while let Err(_) = self.write_progress.compare_exchange(
                    head,
                    head + 1,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {}
                Ok(())
            } else {
                Err(anyhow!("Failed to write"))
            }
        }
    }
}

pub struct SingleConsumer {
    read_progress: AtomicU64,
}

impl Consumer for SingleConsumer {
    fn new() -> SingleConsumer {
        SingleConsumer {
            read_progress: AtomicU64::new(0),
        }
    }
    fn tail(&self) -> u64 {
        self.read_progress.load(Ordering::Relaxed)
    }
    fn pop<T: Copy + Debug>(&self, tail: u64, queue: *mut T) -> Result<T> {
        unsafe {
            let p = queue.offset((tail % BUFFER_SIZE) as isize);
            let res = *p;
            std::sync::atomic::fence(Ordering::SeqCst);
            self.read_progress.fetch_add(1, Ordering::Relaxed);
            Ok(res)
        }
    }
}

pub struct MultiConsumer {
    read_progress: AtomicU64,
}

impl Consumer for MultiConsumer {
    fn new() -> MultiConsumer {
        MultiConsumer {
            read_progress: AtomicU64::new(0),
        }
    }
    fn tail(&self) -> u64 {
        self.read_progress.load(Ordering::Relaxed)
    }
    fn pop<T: Copy>(&self, tail: u64, queue: *mut T) -> Result<T> {
        unsafe {
            let p = queue.offset((tail % BUFFER_SIZE) as isize);
            let res = *p;

            if let Ok(_) = self.read_progress.compare_exchange(
                tail,
                tail + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(res)
            } else {
                Err(anyhow!("Could not read from tail"))
            }
        }
    }
}

pub struct UniqueSender<T> {
    send_closure: Box<dyn Fn(T) -> Result<()>>,
}
pub struct SharedSender<T> {
    send_closure: Box<dyn Fn(T) -> Result<()>>,
}
pub struct UniqueReceiver<T> {
    receive_closure: Box<dyn Fn() -> Result<T>>,
}
pub struct SharedReceiver<T> {
    receive_closure: Box<dyn Fn() -> Result<T>>,
}

pub trait Sender<T> {
    fn send(&self, e: T) -> Result<()>;
    fn new(c: Box<dyn Fn(T) -> Result<()>>) -> Self;
}

pub trait Receiver<T> {
    fn receive(&self) -> Result<T>;
    fn new(c: Box<dyn Fn() -> Result<T>>) -> Self;
}

impl<T> Sender<T> for UniqueSender<T> {
    fn send(&self, e: T) -> Result<()> {
        (self.send_closure)(e)
    }
    fn new(c: Box<dyn Fn(T) -> Result<()>>) -> Self {
        Self { send_closure: c }
    }
}

impl<T> Sender<T> for SharedSender<T> {
    fn send(&self, e: T) -> Result<()> {
        (self.send_closure)(e)
    }
    fn new(c: Box<dyn Fn(T) -> Result<()>>) -> Self {
        Self { send_closure: c }
    }
}

impl<T> Receiver<T> for UniqueReceiver<T> {
    fn receive(&self) -> Result<T> {
        (self.receive_closure)()
    }
    fn new(c: Box<dyn Fn() -> Result<T>>) -> Self {
        Self { receive_closure: c }
    }
}

impl<T> Receiver<T> for SharedReceiver<T> {
    fn receive(&self) -> Result<T> {
        (self.receive_closure)()
    }
    fn new(c: Box<dyn Fn() -> Result<T>>) -> Self {
        Self { receive_closure: c }
    }
}

unsafe impl<T> Send for UniqueSender<T> where T: Copy + Default + Debug {}
unsafe impl<T> Send for UniqueReceiver<T> where T: Copy + Default + Debug {}

unsafe impl<T> Send for SharedSender<T> where T: Copy + Default + Debug {}
unsafe impl<T> Send for SharedReceiver<T> where T: Copy + Default + Debug {}
unsafe impl<T> Sync for SharedSender<T> where T: Copy + Default + Debug {}
unsafe impl<T> Sync for SharedReceiver<T> where T: Copy + Default + Debug {}

pub type SPSC<T> = Queue<T, SingleProducer, SingleConsumer, UniqueSender<T>, UniqueReceiver<T>>;

unsafe impl<T> Send for SPSC<T> where T: Copy + Default + Debug {}
unsafe impl<T> Sync for SPSC<T> where T: Copy + Default + Debug {}

pub type SPMC<T> = Queue<T, SingleProducer, MultiConsumer, UniqueSender<T>, SharedReceiver<T>>;

unsafe impl<T> Send for SPMC<T> where T: Copy + Default + Debug {}
unsafe impl<T> Sync for SPMC<T> where T: Copy + Default + Debug {}

pub type MPSC<T> = Queue<T, MultiProducer, SingleConsumer, SharedSender<T>, UniqueReceiver<T>>;

unsafe impl<T> Send for MPSC<T> where T: Copy + Default + Debug {}
unsafe impl<T> Sync for MPSC<T> where T: Copy + Default + Debug {}

pub type MPMC<T> = Queue<T, MultiProducer, MultiConsumer, SharedSender<T>, SharedReceiver<T>>;

unsafe impl<T> Send for MPMC<T> where T: Copy + Default + Debug {}
unsafe impl<T> Sync for MPMC<T> where T: Copy + Default + Debug {}
