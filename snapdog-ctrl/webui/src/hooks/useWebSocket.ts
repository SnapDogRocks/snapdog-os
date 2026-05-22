import { useEffect, useRef } from "react";

type WsCallback = (data?: unknown) => void;

class WebSocketManager {
  private socket: WebSocket | null = null;
  private listeners: Map<string, Set<WsCallback>> = new Map();
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private reconnectDelay = 1000;
  private maxReconnectDelay = 30000;
  private url: string = "";

  constructor() {
    if (typeof window !== "undefined") {
      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const host = window.location.host;
      this.url = `${protocol}//${host}/api/ws`;
      this.connect();
    }
  }

  private connect() {
    if (this.socket) {
      try {
        this.socket.close();
      } catch { /* ignore */ }
    }

    this.socket = new WebSocket(this.url);

    this.socket.onopen = () => {
      this.reconnectDelay = 1000;
    };

    this.socket.onmessage = (event) => {
      try {
        const eventName = event.data as string;
        this.emit(eventName);
      } catch { /* ignore parse errors */ }
    };

    this.socket.onclose = () => {
      this.scheduleReconnect();
    };

    this.socket.onerror = () => {};
  }

  private scheduleReconnect() {
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
    }
    this.reconnectTimeout = setTimeout(() => {
      this.reconnectDelay = Math.min(this.reconnectDelay * 2, this.maxReconnectDelay);
      this.connect();
    }, this.reconnectDelay);
  }

  public subscribe(event: string, callback: WsCallback): () => void {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }
    this.listeners.get(event)!.add(callback);

    return () => {
      const set = this.listeners.get(event);
      if (set) {
        set.delete(callback);
        if (set.size === 0) {
          this.listeners.delete(event);
        }
      }
    };
  }

  private emit(event: string, data?: unknown) {
    const set = this.listeners.get(event);
    if (set) {
      set.forEach((callback) => {
        try {
          callback(data);
        } catch { /* ignore callback errors */ }
      });
    }
  }
}

let manager: WebSocketManager | null = null;
const getManager = () => {
  if (!manager) {
    manager = new WebSocketManager();
  }
  return manager;
};

export const useWebSocket = (event: string, callback: WsCallback) => {
  const savedCallback = useRef<WsCallback>(callback);

  useEffect(() => {
    savedCallback.current = callback;
  }, [callback]);

  useEffect(() => {
    const wsManager = getManager();
    const tick = (data?: unknown) => savedCallback.current(data);
    const unsubscribe = wsManager.subscribe(event, tick);
    return () => {
      unsubscribe();
    };
  }, [event]);
};
