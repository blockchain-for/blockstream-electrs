pub mod stats;

pub use stats::*;

use std::{io, net::SocketAddr, thread, time::Duration};

use prometheus::{self, Encoder};

pub use prometheus::{
    GaugeVec, Histogram, HistogramOpts, HistogramTimer, HistogramVec, IntCounter as Counter,
    IntCounterVec as CounterVec, IntGauge as Gauge, Opts as MetricOpts,
};

use crate::util::spawn_thread;

pub struct Metrics {
    reg: prometheus::Registry,
    addr: SocketAddr,
}

impl Metrics {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            reg: prometheus::Registry::new(),
            addr,
        }
    }

    pub fn counter(&self, opts: prometheus::Opts) -> Counter {
        let c = Counter::with_opts(opts).unwrap();
        self.reg.register(Box::new(c.clone())).unwrap();

        c
    }

    pub fn counter_vec(&self, opts: prometheus::Opts, labels: &[&str]) -> CounterVec {
        let c = CounterVec::new(opts, labels).unwrap();
        self.reg.register(Box::new(c.clone())).unwrap();
        c
    }

    pub fn gauge(&self, opts: prometheus::Opts) -> Gauge {
        let g = Gauge::with_opts(opts).unwrap();
        self.reg.register(Box::new(g.clone())).unwrap();
        g
    }

    pub fn gauge_vec(&self, opts: prometheus::Opts, labels: &[&str]) -> GaugeVec {
        let g = GaugeVec::new(opts, labels).unwrap();
        self.reg.register(Box::new(g.clone())).unwrap();
        g
    }

    pub fn histogram(&self, opts: prometheus::HistogramOpts) -> Histogram {
        let h = Histogram::with_opts(opts).unwrap();
        self.reg.register(Box::new(h.clone())).unwrap();
        h
    }

    pub fn histogram_vec(&self, opts: prometheus::HistogramOpts, labels: &[&str]) -> HistogramVec {
        let h = HistogramVec::new(opts, labels).unwrap();
        self.reg.register(Box::new(h.clone())).unwrap();
        h
    }

    pub fn start(&self) {
        let server = tiny_http::Server::http(self.addr)
            .unwrap_or_else(|_| panic!("failed to start monitoring HTTP server at {}", self.addr));

        start_process_exporter(self);

        let reg = self.reg.clone();

        spawn_thread("metrics", move || loop {
            if let Err(e) = handle_request(&reg, server.recv()) {
                error!("http error: {}", e);
            }
        });
    }
}

fn start_process_exporter(metrics: &Metrics) {
    let rss = metrics.gauge(MetricOpts::new(
        "process_memory_rss",
        "Resident memory size [bytes]",
    ));
    let cpu = metrics.gauge_vec(
        MetricOpts::new("process_cpu_usage", "CPU usage by this process [seconds]"),
        &["type"],
    );
    let fds = metrics.gauge(MetricOpts::new("process_fs_fds", "# of file descriptors"));
    spawn_thread("exporter", move || loop {
        match parse_stats() {
            Ok(stats) => {
                cpu.with_label_values(&["utime"]).set(stats.utime);
                rss.set(stats.rss as i64);
                fds.set(stats.fds as i64);
            }
            Err(e) => warn!("failed to export stats: {}", e),
        }
        thread::sleep(Duration::from_secs(5));
    });
}

fn handle_request(
    reg: &prometheus::Registry,
    request: io::Result<tiny_http::Request>,
) -> io::Result<()> {
    let request = request?;
    let mut buffer = vec![];

    prometheus::TextEncoder::new()
        .encode(&reg.gather(), &mut buffer)
        .unwrap();

    let response = tiny_http::Response::from_data(buffer);

    request.respond(response)
}
