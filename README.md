# temperqueue

This is a test implementation of four queues built in Temper, my Temporal fuzzing library (see [reitzen.com](https://reitzen.com)). 

It uses unsafe shared memory and is not fit nor intended for production use. 

However, the core algorithm has been heavily fuzzed for correctness. Any bugs are likely to be due to poor translating of the Clojure Temper code into Rust. 

The code uses exclusively Relaxed atomics paired with SeqCst fence() calls. This is to emulate the constructs available in Temper, and is not the correct way to write this code in Rust. Emulating the Rust/C++ memory model will come in an updated version of Temper. 

The queue memory is leaked even when dropped and it does not handle wraparound. This is not a production library, nor is it high quality code. Although it does use types to enforce that single producers or consumers are not Sync.
