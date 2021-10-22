use std::sync::mpsc::*;
use std::sync::Mutex;
use std::sync::Arc;
use std::thread;


pub trait ThreadPool {
    fn new(threads: u32) -> Result<Self, ()>
    where
        Self: Sized;
    
    fn spawn<F>(&self, job: F)
    where 
        F: FnOnce() + Send + 'static;
}


pub enum ThreadPoolMessage {
    Task(Box<dyn FnOnce() + Send + 'static>),
    Shutdown
}

// type Task = Box<dyn FnOnce() + Send + 'static>;

const MAX_THREAD_POOL_SIZE: u32 = 16;

pub struct SharedQueueThreadPool {
    // receiver: Arc<Mutex<Receiver<ThreadPoolMessage>>>,
    sender: Arc<Sender<ThreadPoolMessage>>,
    pool: Vec<Option<thread::JoinHandle<()>>>,
    capacity: u32
}

impl ThreadPool for SharedQueueThreadPool {
    /// 生成线程池
    fn new(threads: u32) -> Result<SharedQueueThreadPool, ()> {
        if threads > MAX_THREAD_POOL_SIZE {
            return Err(())
        }
        let (sender, receiver) = channel::<ThreadPoolMessage>();
        let receiver = Arc::new(Mutex::new(receiver));
        let mut pool = vec![];
        for id in 0..threads {
            let recv = Arc::clone(&receiver);
            let join_handle = thread::spawn(move || {
                run_task(id, recv);
            });
            pool.push(Some(join_handle));
        }
        Ok(Self {
            // receiver,
            sender: Arc::new(sender),
            capacity: threads,
            pool: pool
        })
    }
    
    
    /// 向线程池中传递执行方法
    fn spawn<F>(&self, job: F)
    where F: FnOnce() + Send + 'static {
        let task = ThreadPoolMessage::Task(Box::new(job));
        self.sender.send(task).unwrap();
    }
    
}

impl SharedQueueThreadPool {
    /// 销毁线程池
    pub fn shutdown(&mut self) {
        for _ in 0..self.capacity {
            self.sender.send(ThreadPoolMessage::Shutdown).unwrap();
        }

        for thread in &mut self.pool {
            if let Some(thread) = thread.take() {
                thread.join().unwrap();
            }
        }
    }

}

/// 执行任务方法
pub fn run_task(id: u32, receiver: Arc<Mutex<Receiver<ThreadPoolMessage>>>) {
    loop {
        let recv = receiver.lock().unwrap();
        match recv.recv().unwrap() {
            ThreadPoolMessage::Task(task) => {
                task();
            },

            ThreadPoolMessage::Shutdown => {
                break;
            }
        }
    }
    println!("[Debug] Thread {} exit", id);
}

impl Drop for SharedQueueThreadPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}