import { useEffect, useRef } from "react";

type WsCallback = (data?: any) => void;

class WebSocketManager {
  private socket: WebSocket | null = null;
  private listeners: Map<string, Set<WsCallback>> = new Map();
  private reconnectTimeout: any = null;
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
      } catch (_) {}
    }

    console.log(`Connecting to WebSocket at ${this.url}`);
    this.socket = new WebSocket(this.url);

    this.socket.onopen = () => {
      console.log("WebSocket connected successfully");
      this.reconnectDelay = 1000; // Reset backoff delay
    };

    this.socket.onmessage = (event) => {
      try {
        const eventName = event.data;
        console.log("Received WebSocket event:", eventName);
        this.emit(eventName);
      } catch (e) {
        console.error("Error parsing WebSocket event:", e);
      }
    };

    this.socket.onclose = () => {
      console.warn("WebSocket connection closed. Reconnecting...");
      this.scheduleReconnect();
    };

    this.socket.onerror = (error) => {
      console.error("WebSocket error:", error);
    };
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

    // Return an unsubscribe function
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

  private emit(event: string, data?: any) {
    const set = this.listeners.get(event);
    if (set) {
      set.forEach((callback) => {
        try {
          callback(data);
        } catch (e) {
          console.error("Error invoking WebSocket callback:", e);
        }
      });
    }
  }
}

// Singleton instance
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
    const tick = (data?: any) => savedCallback.current(data);
    const unsubscribe = wsManager.subscribe(event, tick);
    return () => {
      unsubscribe();
    };
  }, [event]);
};
