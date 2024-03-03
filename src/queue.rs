use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};

use crossbeam_epoch::{self as epoch, Atomic, Owned, Shared};

unsafe impl<T: Send> Sync for Queue<T> {}

type Link<T> = Atomic<Node<T>>;

struct Node<T: Send> {
    next: Link<T>,
    data: Option<T>,
}

impl<T: Send> Node<T> {
    fn new(v: T) -> Self {
        Self {
            next: Default::default(),
            data: Some(v),
        }
    }

    fn dummy() -> Self {
        Self {
            next: Atomic::null(),
            data: None,
        }
    }
}

pub struct Queue<T: Send> {
    head: Link<T>,
    tail: Link<T>,
}

impl<T: Send> Queue<T> {
    pub fn new() -> Self {
        let q = Queue {
            head: Atomic::null(),
            tail: Atomic::null(),
        };
        let dummy_node = Owned::new(Node::dummy());

        let guard = unsafe { &epoch::unprotected() };

        let dummy_node = dummy_node.into_shared(guard);
        q.head.store(dummy_node, Relaxed);
        q.tail.store(dummy_node, Relaxed);
        q
    }

    pub fn push(&self, v: T) {
        unsafe { self.try_push(v) }
    }

    unsafe fn try_push(&self, v: T) {
        let guard = &epoch::pin();
        let node = Owned::new(Node::new(v)).into_shared(guard);

        loop {
            let p = self.tail.load(Acquire, guard);

            if (*p.as_raw())
                .next
                .compare_exchange(Shared::null(), node, Release, Relaxed, guard)
                .is_ok()
            {
                let _ = self.tail.compare_exchange(p, node, Release, Relaxed, guard);
                return;
            } else {
                let _ = self.tail.compare_exchange(
                    p,
                    (*p.as_raw()).next.load(Acquire, guard),
                    Release,
                    Relaxed,
                    guard,
                );
            }
        }
    }

    pub fn pop(&self) -> Option<T> {
        unsafe { self.try_pop() }
    }

    unsafe fn try_pop(&self) -> Option<T> {
        let guard = &epoch::pin();

        loop {
            let p = self.head.load(Acquire, guard);

            if (*p.as_raw()).next.load(Acquire, guard).is_null() {
                return None;
            }

            if self
                .head
                .compare_exchange(
                    p,
                    (*p.as_raw()).next.load(Acquire, guard),
                    Release,
                    Relaxed,
                    guard,
                )
                .is_ok()
            {
                let next = (*p.as_raw()).next.load(Acquire, guard).as_raw() as *mut Node<T>;
                return (*next).data.take();
            }
        }
    }

    pub fn peek(&self) -> Option<&T> {
        let guard = &epoch::pin();

        loop {
            let head = self.head.load(Acquire, guard);

            let next = unsafe { (*head.as_raw()).next.load(Acquire, guard) };

            if next.is_null() {
                return None;
            }

            unsafe {
                let data_ref = (*next.as_raw()).data.as_ref();

                match data_ref {
                    Some(data) => return Some(data),
                    None => continue, // No data in this node, try again
                }
            }
        }
    }
}