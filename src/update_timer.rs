use std::{
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    gamma_correction::GammaLookup, opc_pool::OpcPool, pixel_buffer::PixelBuffer,
    screen_samples::ScreenSamples, serial_port::SerialPort, settings::Settings,
};

enum TimerEvent {
    Fired,
    Stopped,
}

struct TimerThread {
    tx: mpsc::Sender<TimerEvent>,
    thread: Option<JoinHandle<()>>,
    throttled: bool,
    stopped: bool,
    throttle_timer: u32,
    delay: u32,
}

impl TimerThread {
    pub fn new(parameters: &Settings, tx: mpsc::Sender<TimerEvent>) -> Self {
        Self {
            tx,
            thread: None,
            throttled: false,
            stopped: false,
            throttle_timer: parameters.throttle_timer,
            delay: parameters.get_delay(),
        }
    }

    pub fn start(timer: Arc<Mutex<TimerThread>>, worker: Arc<Mutex<Option<JoinHandle<()>>>>) {
        let clone = timer.clone();
        let mut timer = timer.lock().expect("lock timer");
        timer.thread = Some(thread::spawn(move || {
            loop {
                let delay = {
                    let timer = clone.lock().expect("lock timer thread");

                    if timer.stopped {
                        timer
                            .tx
                            .send(TimerEvent::Stopped)
                            .expect("send stopped event");
                        break;
                    }

                    timer.tx.send(TimerEvent::Fired).expect("send fired event");

                    if timer.throttled {
                        timer.throttle_timer
                    } else {
                        timer.delay
                    }
                };

                thread::sleep(Duration::from_millis(u64::from(delay)));
            }

            let worker = worker.lock().expect("lock worker thread").take();
            worker.expect("some worker").join().expect("join worker");
        }));
    }

    pub fn stop(timer: Arc<Mutex<TimerThread>>) -> bool {
        let (stopped, thread) = {
            let mut timer = timer.lock().expect("lock timer");

            let stopped = !timer.stopped;
            let thread = timer.thread.take();
            timer.stopped = true;

            (stopped, thread)
        };

        if let Some(thread) = thread {
            thread.join().expect("join timer");
        };

        stopped
    }

    pub fn throttle(timer: Arc<Mutex<TimerThread>>) -> bool {
        let mut timer = timer.lock().expect("lock timer");
        let throttled = timer.throttled;
        timer.throttled = true;
        !throttled && !timer.stopped
    }

    pub fn resume(timer: Arc<Mutex<TimerThread>>) -> bool {
        let mut timer = timer.lock().expect("lock timer");
        let throttled = timer.throttled;
        timer.throttled = false;
        throttled && !timer.stopped
    }
}

struct WorkerThread {
    parameters: Settings,
    rx: mpsc::Receiver<TimerEvent>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl WorkerThread {
    pub fn new(parameters: Settings, rx: mpsc::Receiver<TimerEvent>) -> Self {
        Self {
            parameters,
            rx,
            thread: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(
        timer: Arc<Mutex<TimerThread>>,
        worker: Arc<Mutex<WorkerThread>>,
    ) -> Arc<Mutex<Option<JoinHandle<()>>>> {
        let clone = worker.clone();
        let worker = worker.lock().expect("lock worker");
        let mut thread = worker.thread.lock().expect("lock thread");
        if thread.is_none() {
            *thread = Some(thread::spawn(move || {
                let worker = clone.lock().expect("lock worker thread");
                let gamma = GammaLookup::new();
                let mut samples = ScreenSamples::new(&worker.parameters, &gamma);
                let mut serial_buffer = PixelBuffer::new_serial_buffer(&worker.parameters);
                let mut port = SerialPort::new(&worker.parameters);
                let mut pool = OpcPool::new(&worker.parameters);

                loop {
                    match worker.rx.recv().expect("receive timer event") {
                        TimerEvent::Fired => {
                            if samples.is_empty() {
                                let port_opened = port.open();
                                let pool_opened = pool.open();

                                if (port_opened || pool_opened)
                                    && samples.create_resources().is_ok()
                                {
                                    TimerThread::resume(timer.clone());
                                } else if TimerThread::throttle(timer.clone()) {
                                    serial_buffer.clear();
                                }
                            }

                            if let Err(error) = samples.take_samples() {
                                eprintln!("Samples Error: {:?}", error);
                            }

                            // Update the LED strip.
                            samples.render_serial(&mut serial_buffer);
                            port.send(&serial_buffer);

                            // Send the OPC frames to the server(s).
                            for (i, server) in worker.parameters.servers.iter().enumerate() {
                                for channel in server.channels.iter() {
                                    let mut pixels = if server.alpha_channel {
                                        PixelBuffer::new_bob_buffer(channel)
                                    } else {
                                        PixelBuffer::new_opc_buffer(channel)
                                    };

                                    samples.render_channel(channel, &mut pixels);
                                    pool.send(i, &pixels);
                                }
                            }
                        }
                        TimerEvent::Stopped => {
                            // Reset the LED strip
                            serial_buffer.clear();
                            port.send(&serial_buffer);

                            // Free resources anytime the update timer stops completely.
                            samples.free_resources();
                            port.close();
                            pool.close();
                        }
                    }
                }
            }));
        }

        worker.thread.clone()
    }
}

pub struct UpdateTimer {
    timer: Arc<Mutex<TimerThread>>,
    worker: Arc<Mutex<WorkerThread>>,
}

impl UpdateTimer {
    pub fn new(parameters: Settings) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            timer: Arc::new(Mutex::new(TimerThread::new(&parameters, tx))),
            worker: Arc::new(Mutex::new(WorkerThread::new(parameters, rx))),
        }
    }

    pub fn start(&self) -> bool {
        let worker = WorkerThread::start(self.timer.clone(), self.worker.clone());
        let result = {
            let worker = worker.lock().expect("lock thread");
            worker.is_some()
        };
        if result {
            TimerThread::start(self.timer.clone(), worker);
        }
        result
    }

    pub fn stop(&self) -> bool {
        TimerThread::stop(self.timer.clone())
    }

    pub fn throttle(&self) -> bool {
        TimerThread::throttle(self.timer.clone())
    }

    pub fn resume(&self) -> bool {
        TimerThread::resume(self.timer.clone())
    }
}
