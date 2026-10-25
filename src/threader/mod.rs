use crate::threader::consumer::{Consumer, THREADS_PER_CONSUMER};
use crate::threader::sample::{ParallelSamples, TestResult};
use crate::threader::workload::Workload;
use chrono::{Duration, Utc};
use std::sync::{mpsc, Arc, RwLock};
use std::thread;

mod consumer;
mod sample;
mod worker;
pub mod workload;

pub struct Threader {
    pub num_workers: usize,
    pub max_workers: usize,
    //pub num_samples: u32,
    workload: Workload,
    tx: mpsc::Sender<ParallelSamples>,
    rx: mpsc::Receiver<ParallelSamples>,
    done: Arc<RwLock<bool>>,
    consumers: Vec<Consumer>,
}

impl Threader {
    pub fn new(mut max_workers: usize, workload: Workload) -> Threader {
        if max_workers < 1 {
            max_workers = 1000
        }
        max_workers /= THREADS_PER_CONSUMER as usize;
        max_workers += 1;
        let done = Arc::new(RwLock::new(false));
        let (tx, rx) = mpsc::channel();
        let consumers = Vec::with_capacity(max_workers);
        Threader {
            workload,
            num_workers: 0,
            max_workers,
            //num_samples: 0,
            tx,
            rx,
            done,
            consumers,
        }
    }
    pub fn scaleup(&mut self, new_workers: u32) {
        let mut extra_workers = new_workers - self.num_workers as u32;
        //println!("New worker: {}, extra workers: {}", new_workers, extra_workers);
        if let Some(mut last_consumer) = self.consumers.pop() {
            extra_workers =
                last_consumer.scaleup(extra_workers, self.done.clone(), self.workload.clone());
            self.consumers.push(last_consumer);
        }
        for id in self.consumers.len()..self.max_workers {
            if extra_workers == 0 {
                break;
            }
            let mut new_consumer = Consumer::new(id as u32, self.tx.clone());
            extra_workers =
                new_consumer.scaleup(extra_workers, self.done.clone(), self.workload.clone());
            self.consumers.push(new_consumer);
        }
        self.num_workers = new_workers as usize;
    }
    pub fn finish(&self) {
        if let Ok(mut done) = self.done.clone().write() {
            *done = true;
        }

        // Wait for the workers to finish their last sample, so the table of the default workload
        // isn't dropped while they still use it. Not forever: a query could hang.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !self.consumers.iter().all(|consumer| consumer.stopped())
            && std::time::Instant::now() < deadline
        {
            thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    pub fn wait_stable(
        &mut self,
        spread: f64,
        count: usize,
        max_wait: Duration,
    ) -> Option<TestResult> {
        let end_time = Utc::now() + max_wait;
        let mut parallel_samples = ParallelSamples::new();
        let mut i: usize = 0;
        loop {
            let s = self.consume();
            parallel_samples = parallel_samples.append(&s);
            let test_results = parallel_samples.as_results(count, count + 1);
            //            let stddev = test_result.std_deviation_absolute().unwrap();
            //            println!("tps: {}, latency: {}", stddev.tps, stddev.latency);
            if i > count && Utc::now() > end_time {
                return test_results.mean();
            }
            i += 1;
            if let Some(test_result) = test_results.verify(spread) {
                return Some(test_result);
            }
        }
    }

    fn consume(&mut self) -> ParallelSamples {
        let wait = std::time::Duration::from_millis(10);
        let timeout = std::time::SystemTime::now() + std::time::Duration::from_millis(200);
        let mut parallel_samples = ParallelSamples::new();

        match self.done.read() {
            Ok(_done) => (),
            Err(_err) => (),
        };
        loop {
            //               println!("looping");
            match self.rx.recv_timeout(wait) {
                Ok(pss) => {
                    //        println!("adding");
                    parallel_samples = parallel_samples.append(&pss);
                }
                Err(_err) => (),
            };
            if std::time::SystemTime::now() > timeout {
                break;
            }
        }
        parallel_samples
    }
}
