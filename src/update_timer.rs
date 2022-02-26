use std::{
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    gamma_correction::GammaLookup, opc_pool::OpcPool, pixel_buffer::PixelBuffer,
    screen_samples::ScreenSamples, serial_port::SerialPort, settings::Settings,
};

/// The [TimerThread] runs in a loop firing [TimerEvent] messages over an [std::sync::mpsc]
/// channel to the [WorkerThread].
enum TimerEvent {
    /// The [TimerThread] interval event fired.
    Fired,

    /// The [TimerThread] is stopping.
    Stopped,
}

/// The state and a [JoinHandle<()>] for the [TimerThread].
struct TimerThread {
    /// The [mpsc::Sender<TimerEvent>] to send [TimerEvent] messages to the [WorkerThread].
    tx: mpsc::Sender<TimerEvent>,

    /// The [Option<JoinHandle<()>>] for the [TimerThread], used to join the thread when it
    /// is stopped.
    thread: Option<JoinHandle<()>>,

    /// True if the [TimerThread] is currently throttled because there are no listeners, the
    /// session is locked, or it's a Remote Desktop connection and not connected to the
    /// system console.
    throttled: bool,

    /// True if the [TimerThread] is stopped or stopping.
    stopped: bool,

    /// Time in milliseconds between [TimerThread] loop intervals when throttled.
    throttle_timer: u32,

    /// Time in milliseconds between [TimerThread] loop intervals when not throttled.
    /// This is the time between intervals required to hit the [crate::settings::Settings]
    /// `fps_max` frame rate (`1000 / fps_max`).
    delay: u32,
}

impl TimerThread {
    /// Allocate a new, unstarted [TimerThread] struct.
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

    /// Start the [TimerThread] in `timer`, and pass it the [WorkerThread] [JoinHandle<()>]
    /// in `worker` to let the [TimerThread] join that thread when stopping.
    pub fn start(timer: Arc<Mutex<TimerThread>>, worker: Arc<Mutex<Option<JoinHandle<()>>>>) {
        let clone = timer.clone();
        let mut timer = timer.lock().expect("lock timer");
        timer.stopped = false;
        timer.thread = Some(thread::spawn(move || {
            loop {
                let start_loop = Instant::now();
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
                let next_loop = start_loop + Duration::from_millis(u64::from(delay));
                let start_sleep = Instant::now();
                if next_loop > start_sleep {
                    thread::sleep(next_loop - start_sleep);
                }
            }

            let worker = worker.lock().expect("lock worker thread").take();
            worker.expect("some worker").join().expect("join worker");
        }));
    }

    /// Stop the [TimerThread] in `timer`.
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

    /// Throttle the [TimerThread] in `timer` when the session is locked or
    /// detached from the console, or when there are no listeners.
    pub fn throttle(timer: Arc<Mutex<TimerThread>>) -> bool {
        let mut timer = timer.lock().expect("lock timer");
        let throttled = timer.throttled;
        timer.throttled = true;
        !throttled && !timer.stopped
    }

    /// Resume the throttled [TimerThread] in `timer` when the session is unlocked
    /// or reattaches to the console and there are listeners.
    pub fn resume(timer: Arc<Mutex<TimerThread>>) -> bool {
        let mut timer = timer.lock().expect("lock timer");
        let throttled = timer.throttled;
        timer.throttled = false;
        throttled && !timer.stopped
    }
}

/// The state and a [JoinHandle<()>] for the [WorkerThread].
struct WorkerThread {
    /// Configuration parameters in a [crate::settings::Settings] struct.
    parameters: Settings,

    /// The [mpsc::Receiver<TimerEvent>] to receive [TimerEvent] messages from the [TimerThread].
    rx: mpsc::Receiver<TimerEvent>,

    /// The [Option<JoinHandle<()>>] for the [WorkerThread], used to join the thread when the
    /// [TimerThread] is stopped.
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl WorkerThread {
    /// Allocate a new, unstarted [WorkerThread] struct.
    pub fn new(parameters: Settings, rx: mpsc::Receiver<TimerEvent>) -> Self {
        Self {
            parameters,
            rx,
            thread: Arc::new(Mutex::new(None)),
        }
    }

    /// Start the [WorkerThread] in `worker`, and pass it the [TimerThread]
    /// in `timer` to let the [WorkerThread] throttle and resume the [TimerThread]
    /// when the D3D11 or DXGI resources or the listeners are lost and reconnected.
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

                            let _ = samples.take_samples();

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

                            break;
                        }
                    }
                }
            }));
        }

        worker.thread.clone()
    }
}

/// Public interface which manages the [TimerThread] and [WorkerThread].
pub struct UpdateTimer {
    /// The [TimerThread] instance.
    timer: Arc<Mutex<TimerThread>>,

    /// The [WorkerThread] instance.
    worker: Arc<Mutex<WorkerThread>>,
}

impl UpdateTimer {
    /// Allocate an unstarted [UpdateTimer] using the [Settings] in `parameters`.
    pub fn new(parameters: Settings) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            timer: Arc::new(Mutex::new(TimerThread::new(&parameters, tx))),
            worker: Arc::new(Mutex::new(WorkerThread::new(parameters, rx))),
        }
    }

    /// Start the [WorkerThread] and [TimerThread].
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

    /// Stop the [WorkerThread] and [TimerThread].
    pub fn stop(&self) -> bool {
        TimerThread::stop(self.timer.clone())
    }

    pub fn resume(&self) -> bool {
        TimerThread::resume(self.timer.clone())
    }
}
