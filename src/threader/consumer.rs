use crate::threader::sample::ParallelSamples;
use crate::threader::worker::Worker;
use crate::threader::workload::Workload;
use std::sync::{mpsc, Arc, RwLock};
use std::thread;

pub const THREADS_PER_CONSUMER: i32 = 10;
const SCALEDOWNFACTOR: i32 = 10;

pub struct Consumer {
    id: u32,
    num_threads: u32,
    upstream: mpsc::Sender<ParallelSamples>,
    threads: Vec<thread::JoinHandle<()>>,
}

impl Consumer {
    pub fn new(id: u32, downstream: mpsc::Sender<ParallelSamples>) -> Consumer {
        let done = Arc::new(RwLock::new(false));
        let (upstream, rx) = mpsc::channel();
        let threads = Vec::with_capacity(25);
        //println!("Started new consumer: {}", id);

        thread::Builder::new()
            .name(format!("consumer {}", id).to_string())
            .spawn(move || {
                consumer(rx, downstream, done).unwrap();
            })
            .unwrap();
        Consumer {
            id,
            num_threads: 0,
            upstream,
            threads,
        }
    }
    pub fn scaleup(
        &mut self,
        mut extra_threads: u32,
        done: std::sync::Arc<std::sync::RwLock<bool>>,
        workload: Workload,
    ) -> u32 {
        let mut thread_handle: thread::JoinHandle<()>;
        let mut leftover: i32 = (self.num_threads + extra_threads) as i32 - THREADS_PER_CONSUMER;
        if leftover < 0 {
            leftover = 0
        }
        extra_threads -= leftover as u32;
        let start: u32 = self.id * THREADS_PER_CONSUMER as u32 + self.num_threads;
        let end: u32 = start + extra_threads;

        for thread_id in start..end {
            let workload: Workload = workload.clone();
            let upstream = self.upstream.clone();
            let thread_done = done.clone();
            thread_handle = thread::Builder::new()
                .name(format!("worker {}", thread_id).to_string())
                .spawn(move || {
                    if let Err(err) =
                        Worker::new(thread_id, upstream, thread_done, workload).procedure()
                    {
                        eprintln!("error: worker {}: {}", thread_id, err);
                    }
                })
                .unwrap();
            self.threads.push(thread_handle);
            thread::sleep(std::time::Duration::from_millis(10));
        }
        self.num_threads += extra_threads;
        leftover as u32
    }
}

fn consumer(
    rx: mpsc::Receiver<ParallelSamples>,
    tx: mpsc::Sender<ParallelSamples>,
    done: Arc<RwLock<bool>>,
) -> Result<(), Box<dyn std::error::Error>> {
    //With more threads (> 500) we have some issues, where the one main thread cannot consume messages fast enough.
    //This function can downscal from 25 messages to 1 message.
    let mut parallelsamples = ParallelSamples::new();
    let wait = std::time::Duration::from_millis(10);
    loop {
        match done.read() {
            Ok(done) => {
                if *done {
                    break;
                }
            }
            Err(_err) => (),
        };
        for _ in 0..THREADS_PER_CONSUMER * SCALEDOWNFACTOR {
            match rx.recv_timeout(wait) {
                Ok(sample) => {
                    parallelsamples = parallelsamples.append(&sample);
                }
                Err(_err) => (),
                //                {
                //                    if i%10 == 0 {
                //                        println!("Timeout");
                //                    }
                //                }
            };
        }
        if parallelsamples.len() > 0 {
            tx.send(parallelsamples)?;
            parallelsamples = ParallelSamples::new();
        }
    }
    Ok(())
}
