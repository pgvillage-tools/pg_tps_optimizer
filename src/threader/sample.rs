/*
This module helps to go from one sample (one timeslice, single thread) to
a TestResult (TPS and average latency) once 'it is stable'.
Therefore we have multiple structures. How it works (and some definitions):

* Every thread tries to run as many queries as it can in 200ms and keeps track of duration,
  number of transactions and total waits (from run query to get results). With this:
  * The struct where we track duration, tps and waits is called a Sample.
  * A period of 200ms is called a timeslice. Every Sample belongs to a specific timeslice.
* When it has finished a Sample, it is sent to the master thread with a MPSC channel.
* The master thread collects all samples and combines them into a ParallelSample.
  A ParallelSample is a container for all Samples of a specific timeslice combined.
  As such a ParallelSample can be seen as the total performance on that timeslice.
  It only holds the number of Samples, sum of transactions, sum of waits and sum of duration.
  When the program runs 20 threads, 1000 TPS and latency of 10ms,
  the ParallelSample would look something like
  ParallelSample{
    timeslice: 8414426000, # This is the first timeslice of April 30th, 2023 at 22:06:39
    total_transactions: 200, # 1000TS is 200 transactions in 200ms
    total_waits: 2000ms, # For 200 transactions with latency 10ms, we expect total of 2000ms
    total_duration: 4000ms, # We expect 200ms has elapsed for 20 threads, so total of 4000ms
    num_samples: 20, # For 20 threads we expect 20 samples
    }
* ParallelSamples can collect data (Samples can be added until we have all of them).
  Once we expect we have all of them, we freeze the info into a TestResult.
  A TestResult still holds a combination of all samples for a TimeSlice, but
  the link to the exact timeslice is left out. We keep multiple TestResults together
  and calculate standard deviation. Once we have enough samples, and stddev is
  within parameters, we return a summary (mean TPS and mean latency) as a final TestResult.
*/

use std::convert::TryFrom;
use std::vec::Vec;
use std::{collections::BTreeMap, iter::FromIterator};

use chrono::{DateTime, Duration, TimeZone, Utc};
use num::integer::Roots;

// A sample is one thread trying to run as many transactions as possible
// for 100msec and keeping track of results
pub struct Sample {
    transactions: u64,
    wait: Duration,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

fn timeslice(when: DateTime<Utc>) -> u32 {
    ((when - Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 0).unwrap()).num_milliseconds() / 200) as u32
}

fn current_timeslice() -> u32 {
    timeslice(chrono::Utc::now())
}

fn percent_of(first: f64, second: f64) -> f64 {
    if first == 0.0 {
        return 0.0;
    }
    100.0 * second / first
}

/* Duration supports a div by i32, but sometimes we have an overflow if we use that option*/
fn div_duration(d: Duration, x: u64) -> Duration {
    const MAX_U32: u64 = i32::MAX as u64;
    if x == 0 {
        Duration::zero()
    } else if x > MAX_U32 {
        match i32::try_from(x.sqrt()).ok() {
            Some(rt) => return d / rt,
            None => panic!("that is some unexpected divisor: {}", x),
        }
    } else {
        d / (x as i32)
    }
}

impl Copy for Sample {}

impl Clone for Sample {
    fn clone(&self) -> Sample {
        *self
    }
}

impl Sample {
    // initialize a new sample with no data
    pub fn new() -> Sample {
        Sample {
            transactions: 0,
            wait: Duration::zero(),
            start: chrono::Utc::now(),
            end: chrono::Utc::now(),
        }
    }
    // add a transaction (with the duration of it)
    pub fn increment(&mut self, wait: Duration) {
        self.transactions += 1;
        self.wait += wait;
    }
    // stop sampling
    pub fn end(&mut self) {
        self.end = chrono::Utc::now();
        //println!("{}", (self.end-self.start).num_microseconds().unwrap_or(0));
    }
    // how many transactions did we process per second
    pub fn tps(self) -> f64 {
        let duration: f64 = (self.end - self.start).num_nanoseconds().unwrap() as f64;
        1e9_f64 * (self.transactions as f64) / duration
    }
    /*
    // how many seconds did we waited for a transaction to return
    pub fn waits(self) -> Duration {
        self.end-self.start
    }
    // what latency did we perceive (on average)
    pub fn avg_latency(self) -> Duration {
        let num = self.transactions as i32;
        (self.end-self.start)/num
    }
    */
    // You can materialize a Sample into A ParallelSample struct
    pub fn to_parallel_sample(self) -> ParallelSample {
        //println!("total_waits: {}, transactions: {}", self.wait.num_microseconds().unwrap_or(0), self.transactions);
        ParallelSample {
            timeslice: timeslice(self.start),
            total_transactions: self.transactions,
            total_waits: self.wait,
            total_duration: self.end - self.start,
            num_samples: 1,
        }
    }
}

// ParallelSample are meant as a set of multiple samples within the same period
// run on multiple threads. For efficiency it has a totally different memory structure,
// which only has the summaries data from all added samples.
pub struct ParallelSample {
    pub timeslice: u32,
    total_transactions: u64,
    total_waits: Duration,
    total_duration: Duration,
    pub num_samples: u64,
}

impl Copy for ParallelSample {}

impl Clone for ParallelSample {
    fn clone(&self) -> ParallelSample {
        *self
    }
}

impl ParallelSample {
    // avg latency is the average amount of waits over all samples contained
    pub fn avg_latency(&self) -> Duration {
        div_duration(self.total_waits, self.total_transactions)
    }
    /*
    // initialize a new without data
    pub fn new(timeslice: u32) -> ParallelSample {
        ParallelSample{
            timeslice,
            total_transactions: 0,
            total_waits: Duration::zero(),
            total_duration: Duration::zero(),
            num_samples: 0,
        }
    }
    */
    // Combine two ParallelSample (same time slice, different threads) into one
    pub fn add(&mut self, samples: ParallelSample) -> Result<(), &'static str> {
        if self.timeslice != samples.timeslice {
            return Err("trying to combine samples of different timeslices");
        }
        self.total_transactions += samples.total_transactions;
        self.total_waits += samples.total_waits;
        self.total_duration += samples.total_duration;
        self.num_samples += samples.num_samples;
        Ok(())
    }

    // tot_tps is a sum of all tps's from all samples expecting they where
    // running simultaneously on seperate threads
    pub fn tot_tps(&self) -> f64 {
        if self.num_samples < 1 {
            return 0.0;
        }
        let duration_ns: f64 = div_duration(self.total_duration, self.num_samples)
            .num_nanoseconds()
            .unwrap() as f64;
        match duration_ns < 1_f64 {
            true => 0_f64,
            false => 1e9_f64 * (self.total_transactions as f64) / duration_ns,
        }
    }
    pub fn as_testresult(&self) -> TestResult {
        TestResult {
            stable: false,
            tps: self.tot_tps(),
            latency: self.avg_latency(),
        }
    }
}

pub struct ParallelSamples {
    parallel_samples: BTreeMap<u32, ParallelSample>,
    iterator_keys: Vec<u32>,
    current: usize,
}

impl Clone for ParallelSamples {
    fn clone(&self) -> ParallelSamples {
        let mut pss = ParallelSamples::new();
        for (i, ps) in &self.parallel_samples {
            pss.parallel_samples.insert(*i, *ps);
        }
        pss
    }
}

impl Iterator for ParallelSamples {
    type Item = ParallelSample;
    fn next(&mut self) -> Option<Self::Item> {
        if self.parallel_samples.len() != self.iterator_keys.len() {
            self.iterator_keys = Vec::from_iter(self.parallel_samples.keys().copied());
            self.iterator_keys.sort();
            self.current = 0
        }
        if self.current >= self.parallel_samples.len() {
            return None;
        }
        match self.iterator_keys.get(self.current) {
            Some(current_key) => match self.parallel_samples.get(current_key) {
                Some(parallel_sample) => {
                    self.current += 1;
                    Some(*parallel_sample)
                }
                None => None,
            },
            None => None,
        }
    }
}

impl ParallelSamples {
    // initialize a new without data
    pub fn new() -> ParallelSamples {
        ParallelSamples {
            parallel_samples: BTreeMap::new(),
            iterator_keys: Vec::new(),
            current: 0,
        }
    }
    pub fn add(&mut self, sample: ParallelSample) {
        self.parallel_samples
            .entry(sample.timeslice)
            .and_modify(|s| s.add(sample).unwrap())
            .or_insert(sample);
    }
    pub fn len(&self) -> usize {
        self.parallel_samples.len()
    }
    pub fn limit(mut self, limit: usize) -> ParallelSamples {
        for _ in limit..self.len() {
            self.parallel_samples.pop_first();
        }
        self
    }
    pub fn append(mut self, samples: &ParallelSamples) -> ParallelSamples {
        for sample in samples.parallel_samples.values() {
            self.add(*sample);
        }
        self.limit(100)
    }
    pub fn as_results(&self, min: usize, max: usize) -> TestResults {
        let previous_timeslice = current_timeslice() - 1;
        let mut results = TestResults::new(min, max);
        for (_, parallel_sample) in self.parallel_samples.clone() {
            if parallel_sample.timeslice >= previous_timeslice {
                break;
            }
            results.append(parallel_sample.as_testresult());
        }
        results
    }
}

pub struct TestResult {
    pub stable: bool,
    pub tps: f64,
    pub latency: Duration,
}

impl Copy for TestResult {}

impl Clone for TestResult {
    fn clone(&self) -> TestResult {
        *self
    }
}
pub struct TestResults {
    pub min: usize,
    max: usize,
    results: Vec<TestResult>,
}

impl TestResults {
    pub fn new(min: usize, max: usize) -> TestResults {
        TestResults {
            min,
            max,
            results: Vec::new(),
        }
    }
    fn tot_tps(&self) -> f64 {
        self.results.iter().map(|tr| tr.tps).sum::<f64>()
    }
    fn avg_latency(&self) -> Duration {
        // I wished I could do something like this instead:
        // self.results.iter().map(|tr| tr.latency).sum::<Duration>();
        // But I get `the trait bound `chrono::Duration: Sum` is not satisfied`
        let mut num: u64 = 0;
        let mut tot_lat = Duration::zero();
        for tr in self.results.clone() {
            tot_lat += tr.latency;
            num += 1
        }
        match num {
            0 => tot_lat,
            _ => div_duration(tot_lat, num),
        }
    }
    fn len(&self) -> usize {
        self.results.len()
    }
    pub fn mean(&self) -> Option<TestResult> {
        let sum_tps = self.tot_tps();
        let avg_latency = self.avg_latency();
        let count = self.len();

        match count {
            positive if positive > 0 => Some(TestResult {
                stable: false,
                tps: sum_tps / (count as f64),
                latency: avg_latency,
            }),
            _ => None,
        }
    }

    pub fn std_deviation_absolute(&self) -> Option<TestResult> {
        match (self.mean(), self.results.len()) {
            (Some(results), count) if count > 0 => {
                let tps_variance = self
                    .results
                    .iter()
                    .map(|tr| {
                        let tps_diff = results.tps - tr.tps;
                        tps_diff * tps_diff
                    })
                    .sum::<f64>()
                    / count as f64;
                let lat_variance = self
                    .results
                    .iter()
                    .map(|tr| {
                        let lat_diff = (results.latency - tr.latency)
                            .num_microseconds()
                            .unwrap_or(0) as f64;
                        lat_diff * lat_diff
                    })
                    .sum::<f64>()
                    / count as f64;

                Some(TestResult {
                    stable: false,
                    tps: tps_variance.sqrt(),
                    latency: Duration::microseconds(lat_variance.sqrt() as i64),
                })
            }
            _ => None,
        }
    }
    /*
    pub fn clear(&mut self) {
        self.results.clear();
    }
    */
    pub fn append(&mut self, result: TestResult) {
        self.results.insert(self.results.len(), result);
        if self.results.len() > self.max {
            self.results.remove(0);
        }
    }
    pub fn verify(&self, spread: f64) -> Option<TestResult> {
        if self.results.len() < self.min {
            return None;
        }
        match (self.std_deviation_absolute(), self.mean()) {
            (Some(stdev), Some(mut mean)) => {
                if !((0.0..spread).contains(&percent_of(mean.tps, stdev.tps))
                    && (0.0..spread).contains(&percent_of(
                        mean.latency.num_microseconds().unwrap_or(0) as f64,
                        stdev.latency.num_microseconds().unwrap_or(0) as f64,
                    )))
                {
                    None
                } else {
                    mean.stable = true;
                    Some(mean)
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use range_check::Check;
    use std::{thread, time};
    const NUM_TRANSACTIONS: usize = 36;
    const NUM_THREADS: usize = 88;
    const TIMESLICES_PER_SECOND: usize = 5;
    const NUM_TIMESLICES: usize = 10;
    const WAIT_MS: i64 = 5;

    impl TestResults {
        fn avg_tps(&self) -> f64 {
            if self.len() == 0 {
                return 0.0;
            }
            self.tot_tps() / (self.len() as f64)
        }
    }

    fn create_test_sample(num_transaction: usize, wait: Duration) -> Sample {
        let mut sample = Sample::new();
        for _ in 1..num_transaction {
            sample.increment(wait);
        }
        thread::sleep(time::Duration::from_millis(200));
        sample.end();
        sample
    }
    fn create_test_parasample(sample: Sample, num_threads: usize) -> ParallelSample {
        let mut ps = sample.to_parallel_sample();
        for _ in 1..num_threads {
            _ = ps.add(sample.to_parallel_sample());
        }
        ps
    }
    fn create_test_parasamples(
        mut ps: ParallelSample,
        from_ts: u32,
        num_ts: usize,
        increase: u64,
    ) -> ParallelSamples {
        let mut pps = ParallelSamples::new();
        for slice in from_ts..(from_ts + num_ts as u32) {
            ps.timeslice = slice;
            ps.num_samples += increase;
            pps.add(ps.clone());
        }
        pps
    }
    fn percent_of(first: f64, second: f64) -> f64 {
        if first == 0.0 {
            return 0.0;
        }
        return 100.0 * second / first;
    }
    #[test]
    fn test_percent_of() {
        assert_eq!(percent_of(0.0, 50.0), 0.0);
        assert_eq!(percent_of(50.0, 50.0), 100.0);
        assert_eq!(percent_of(100.0, 50.0), 50.0);
        assert_eq!(percent_of(-100.0, 50.0), -50.0);
        assert_eq!(percent_of(-100.0, -50.0), 50.0);
        assert_eq!(percent_of(-10.0, -50.0), 500.0);
    }
    #[test]
    fn test_sample() {
        let sample = create_test_sample(NUM_TRANSACTIONS, Duration::milliseconds(WAIT_MS));
        let s_tps = sample.clone().tps();
        assert!(s_tps < 180_f64);

        let ms = sample.to_parallel_sample();
        assert_eq!(s_tps, ms.tot_tps());
        assert_eq!(ms.avg_latency().num_microseconds().unwrap(), 5000);
    }
    #[test]
    fn test_parallel_sample() {
        let sample = create_test_sample(NUM_TRANSACTIONS, Duration::milliseconds(WAIT_MS));
        let ps = create_test_parasample(sample, NUM_THREADS);
        let mut other = ps.clone();
        other.timeslice += 1;
        assert_eq!(
            other.add(ps).unwrap_err(),
            "trying to combine samples of different timeslices"
        );
        let percent = percent_of(
            ps.tot_tps(),
            (NUM_TRANSACTIONS * NUM_THREADS * TIMESLICES_PER_SECOND) as f64,
        );
        assert_eq!(percent.check_range(90.0..110.0), Ok(percent));
        let avg_latency = ps.avg_latency().num_microseconds().unwrap();
        assert!(avg_latency <= 5010 && avg_latency > 4990);
    }
    #[test]
    fn test_parallel_samples() {
        let sample = create_test_sample(NUM_TRANSACTIONS, Duration::milliseconds(WAIT_MS));
        let ps = create_test_parasample(sample, NUM_THREADS);
        let mut pss = ParallelSamples::new();
        pss.add(ps);
        let mut other = ps.clone();
        let mut other_pss = ParallelSamples::new();
        other_pss.add(other);
        for i in 1..101 {
            assert_eq!(pss.len(), i);
            other.timeslice += 1;
            other_pss = ParallelSamples::new();
            other_pss.add(other);
            pss = pss.clone().append(&other_pss);
        }
        assert_eq!(pss.len(), 100);
        pss = pss.limit(5);
        assert_eq!(pss.len(), 5);
        pss = pss.limit(100);
        assert_eq!(pss.count(), 5);
    }
    #[test]
    fn test_results() {
        let expected_tps = (NUM_TRANSACTIONS * NUM_THREADS * TIMESLICES_PER_SECOND) as f64;
        let expected_latency = Duration::milliseconds(WAIT_MS);
        let sample = create_test_parasample(
            create_test_sample(NUM_TRANSACTIONS, expected_latency),
            NUM_THREADS,
        );
        let mut pps = create_test_parasamples(sample, current_timeslice(), NUM_TIMESLICES, 10);
        let mut results = pps.as_results(1, NUM_TIMESLICES);
        // Since we start at current timeslice, we expect we get no results
        assert_eq!(results.len(), 0);
        assert_eq!(results.tot_tps(), 0_f64);
        assert_eq!(results.avg_tps(), 0_f64);
        assert_eq!(results.avg_latency().num_microseconds().unwrap(), 0);

        pps = create_test_parasamples(sample, current_timeslice() - 20, NUM_TIMESLICES + 1, 1);
        results = pps.as_results(100, NUM_TIMESLICES);
        assert_eq!(results.len(), NUM_TIMESLICES);
        let mut percent = percent_of(results.avg_tps(), expected_tps);
        assert_eq!(percent.check_range(90.0..110.0), Ok(percent));
        percent = percent_of(
            results.avg_latency().num_microseconds().unwrap() as f64,
            expected_latency.num_microseconds().unwrap() as f64,
        );
        assert_eq!(percent.check_range(90.0..110.0), Ok(percent));
        assert!(results.verify(5.0).is_none());
        results.min = 1;
        let mean = results.mean().unwrap();
        println!("mean: {} {}", mean.tps, mean.latency.num_milliseconds());
        assert!(mean.tps > 0.0);
        assert!(mean.latency.num_milliseconds() > 0);
        let stdev = results.std_deviation_absolute().unwrap();
        println!("stdev: {} {}", stdev.tps, stdev.latency.num_milliseconds());
        assert!(results.verify(5.0).is_some());
        let mean = results.mean().unwrap().clone();
        percent = percent_of(mean.tps, expected_tps);
        assert_eq!(percent.check_range(90.0..110.0), Ok(percent));
        percent = percent_of(
            mean.latency.num_microseconds().unwrap() as f64,
            expected_latency.num_microseconds().unwrap() as f64,
        );
        assert_eq!(percent.check_range(90.0..110.0), Ok(percent));
    }
}
