use crate::queue::{
    Receiver, Sender, SharedReceiver, SharedSender, UniqueReceiver, UniqueSender, MPMC, MPSC, SPSC,
};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use num_cpus;


mod queue;

fn main() {}

#[cfg(test)]
mod test {

    const TO_READ: i32 = 1_000_000;

    use super::*;
    use crate::queue::SPMC;

    fn test_shared_sender(s: SharedSender<i32>, thread_count: usize) -> JoinHandle<()> {
        thread::spawn(move || {
            let mut threads = Vec::new();
            let a = Arc::new(s);
            for _ in 0..thread_count {
                let a = a.clone();
                threads.push(thread::spawn(move || {
                    for c in 0..TO_READ {
                        while let Err(_) = a.send(c) {}
                    }
                }));
            }

            for t in threads {
                t.join().unwrap();
            }
        })
    }

    fn test_unique_sender(s: UniqueSender<i32>) -> JoinHandle<()> {
        thread::spawn(move || {
            for c in 0..TO_READ {
                while let Err(_) = s.send(c) {}
            }
        })
    }

    fn test_unique_receiver(r: UniqueReceiver<i32>, halt: Arc<Mutex<bool>>) -> JoinHandle<i64> {
        thread::spawn(move || {
            let mut res = 0 as i64;
            loop {
                if *halt.lock().unwrap() {
                    break res;
                }

                if let Ok(v) = r.receive() {
                    res += v as i64;
                }
            }
        })
    }

    fn test_shared_receiver(
        r: SharedReceiver<i32>,
        thread_count: usize,
        halt: Arc<Mutex<bool>>,
    ) -> JoinHandle<i64> {
        thread::spawn(move || {
            let mut threads = Vec::new();
            let a = Arc::new(r);
            for _ in 0..thread_count {
                let a = a.clone();
                let halt = halt.clone();
                threads.push(thread::spawn(move || {
                    let mut res = 0;
                    loop {
                        if *halt.lock().unwrap() {
                            break res;
                        }

                        if let Ok(v) = a.receive() {
                            res += v as i64;
                        }
                    }
                }));
            }

            let mut out = 0 as i64;
            for t in threads {
                out += t.join().unwrap();
            }
            out
        })
    }

    #[test]
    fn test_spsc() {
        let (send, recv) = SPSC::<i32>::channel();

        let finished = Arc::new(Mutex::new(false));
        let send_test = test_unique_sender(send);
        let receive_test = test_unique_receiver(recv, finished.clone());

        send_test.join().unwrap();

        {
            let mut f = finished.lock().unwrap();
            *f = true;
        }

        let r = receive_test.join().unwrap();
        let tr = TO_READ as i64 - 1;

        let expected = (tr * tr) / 2 + (tr + 1) / 2;
        assert!(r == expected);
    }

    #[test]
    fn test_mpsc() {
        let (send, recv) = MPSC::<i32>::channel();
        let send_thread_count = num_cpus::get() - 1;

        let finished = Arc::new(Mutex::new(false));
        let send_test = test_shared_sender(send, send_thread_count);
        let receive_test = test_unique_receiver(recv, finished.clone());

        send_test.join().unwrap();

        {
            let mut f = finished.lock().unwrap();
            *f = true;
        }

        let r = receive_test.join().unwrap();
        let tr = TO_READ as i64 - 1;

        let expected = ((tr * tr) / 2 + (tr + 1) / 2) * send_thread_count as i64;
        assert!(r == expected);
    }

    #[test]
    fn test_spmc() {
        let (send, recv) = SPMC::<i32>::channel();
        let receive_thread_count  = num_cpus::get() - 1;

        let finished = Arc::new(Mutex::new(false));
        let send_test = test_unique_sender(send);
        let receive_test = test_shared_receiver(recv, receive_thread_count, finished.clone());

        send_test.join().unwrap();

        {
            let mut f = finished.lock().unwrap();
            *f = true;
        }

        let r = receive_test.join().unwrap();
        let tr = TO_READ as i64 - 1;

        let expected = (tr * tr) / 2 + (tr + 1) / 2;
        assert!(r == expected);
    }

    #[test]
    fn test_mpmc() {
        let (send, recv) = MPMC::<i32>::channel();
        let thread_count = num_cpus::get() / 2;

        let finished = Arc::new(Mutex::new(false));
        let send_test = test_shared_sender(send, thread_count);
        let receive_test = test_shared_receiver(recv, thread_count, finished.clone());

        send_test.join().unwrap();

        {
            let mut f = finished.lock().unwrap();
            *f = true;
        }

        let r = receive_test.join().unwrap();
        let tr = TO_READ as i64 - 1;

        let expected = ((tr * tr) / 2 + (tr + 1) / 2) * thread_count as i64;
        assert!(r == expected);
    }
}
