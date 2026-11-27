package example;

import java.util.concurrent.locks.Lock;
import java.util.concurrent.locks.ReentrantLock;
import java.util.concurrent.locks.ReentrantReadWriteLock;

class Locks {
    private final ReentrantLock lock = new ReentrantLock();
    private final ReentrantReadWriteLock cache = new ReentrantReadWriteLock();
    private final Door door = new Door();

    // An exception in work() keeps these locks held.
    void parameter(Lock guard) {
        guard.lock(); // expect: java.lock-without-finally
        work();
        guard.unlock();
    }

    void field() throws InterruptedException {
        lock.lockInterruptibly(); // expect: java.lock-without-finally
        work();
        lock.unlock();
    }

    void write() {
        cache.writeLock().lock(); // expect: java.lock-without-finally
        work();
        cache.writeLock().unlock();
    }

    // The lock is released in finally.
    void guarded() {
        lock.lock();
        try {
            work();
        } finally {
            lock.unlock();
        }
    }

    void read() {
        cache.readLock().lock();
        try {
            work();
        } finally {
            cache.readLock().unlock();
        }
    }

    void inside() {
        try {
            lock.lock();
            work();
        } finally {
            lock.unlock();
        }
    }

    // Another type's lock() method is not a java.util.concurrent lock.
    void close() {
        door.lock();
    }

    void work() {}
}

class Door {
    void lock() {}
}
