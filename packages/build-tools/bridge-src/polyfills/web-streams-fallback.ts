var FallbackWritableStream = class {
  constructor(sink = {}) {
    this._sink = sink;
  }
  getWriter() {
    const sink = this._sink;
    return {
      write(chunk) {
        return Promise.resolve(typeof sink.write === "function" ? sink.write(chunk) : void 0);
      },
      close() {
        return Promise.resolve(typeof sink.close === "function" ? sink.close() : void 0);
      },
      releaseLock() {
      }
    };
  }
};
var FallbackReadableStream = class {
  constructor(source = {}) {
    this._queue = [];
    this._pending = [];
    this._closed = false;
    this._error = null;
    const flushPending = () => {
      while (this._pending.length > 0) {
        const waiter = this._pending.shift();
        if (this._error) {
          waiter.reject(this._error);
          continue;
        }
        if (this._queue.length > 0) {
          waiter.resolve({ value: this._queue.shift(), done: false });
          continue;
        }
        if (this._closed) {
          waiter.resolve({ value: void 0, done: true });
          continue;
        }
        this._pending.unshift(waiter);
        break;
      }
    };
    const controller = {
      enqueue: (value) => {
        if (this._closed || this._error) return;
        this._queue.push(value);
        flushPending();
      },
      close: () => {
        if (this._closed || this._error) return;
        this._closed = true;
        flushPending();
      },
      error: (error) => {
        if (this._closed || this._error) return;
        this._error = error instanceof Error ? error : new Error(String(error));
        flushPending();
      }
    };
    if (typeof source.start === "function") {
      Promise.resolve().then(() => source.start(controller)).catch((error) => controller.error(error));
    }
  }
  getReader() {
    return {
      read: () => {
        if (this._error) {
          return Promise.reject(this._error);
        }
        if (this._queue.length > 0) {
          return Promise.resolve({ value: this._queue.shift(), done: false });
        }
        if (this._closed) {
          return Promise.resolve({ value: void 0, done: true });
        }
        return new Promise((resolve, reject) => {
          this._pending.push({ resolve, reject });
        });
      },
      releaseLock() {
      }
    };
  }
};

export { FallbackWritableStream, FallbackReadableStream };
