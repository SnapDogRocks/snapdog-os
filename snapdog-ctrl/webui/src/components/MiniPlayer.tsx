"use client";

import { useState, useEffect, useRef, useCallback } from "react";
import { api, type NowPlaying } from "@/lib/api";
import { useWebSocket } from "@/hooks/useWebSocket";
import { useTranslations } from "next-intl";

function formatTime(ms: number): string {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const sec = s % 60;
  return `${m}:${sec.toString().padStart(2, "0")}`;
}

// ── Icons ─────────────────────────────────────────────────────

function PlayIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="currentColor">
      <path d="M8 5v14l11-7z" />
    </svg>
  );
}

function PauseIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="currentColor">
      <path d="M6 4h4v16H6zM14 4h4v16h-4z" />
    </svg>
  );
}

function NextIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="currentColor">
      <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
    </svg>
  );
}

function PrevIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="currentColor">
      <path d="M6 6h2v12H6zM9.5 12l8.5 6V6z" />
    </svg>
  );
}

function ChevronDownIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.5} strokeLinecap="round" strokeLinejoin="round">
      <path d="M6 9l6 6 6-6" />
    </svg>
  );
}

function VolumeIcon({ muted, className }: { muted: boolean; className?: string }) {
  if (muted) {
    return (
      <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
        <path d="M11 5L6 9H2v6h4l5 4V5z" />
        <line x1="23" y1="9" x2="17" y2="15" />
        <line x1="17" y1="9" x2="23" y2="15" />
      </svg>
    );
  }
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
      <path d="M11 5L6 9H2v6h4l5 4V5z" />
      <path d="M19.07 4.93a10 10 0 010 14.14M15.54 8.46a5 5 0 010 7.08" />
    </svg>
  );
}

// ── NowPlayingSheet ───────────────────────────────────────────

function NowPlayingSheet({ np, open, onClose }: { np: NowPlaying; open: boolean; onClose: () => void }) {
  const t = useTranslations("player");
  const [localPosition, setLocalPosition] = useState(np.position_ms);
  const [dragging, setDragging] = useState(false);
  const [localVolume, setLocalVolume] = useState(np.volume);
  const [volumeDragging, setVolumeDragging] = useState(false);
  const seekRef = useRef<HTMLInputElement>(null);

  // Sync position/volume from server when not dragging
  const positionSource = dragging ? localPosition : np.position_ms;
  const volumeSource = volumeDragging ? localVolume : np.volume;

  // Tick position forward while playing
  useEffect(() => {
    if (!np.playing || dragging) return;
    const interval = setInterval(() => {
      setLocalPosition((p) => Math.min(p + 1000, np.duration_ms));
    }, 1000);
    return () => clearInterval(interval);
  }, [np.playing, np.duration_ms, dragging]);

  return (
    <div
      className={`fixed inset-0 z-50 flex flex-col transition-transform duration-300 ease-out motion-reduce:transition-none ${open ? "translate-y-0" : "translate-y-full"}`}
      role="dialog"
      aria-modal="true"
      aria-label={t("nowPlaying")}
      aria-hidden={!open}
    >
      {/* Backdrop */}
      <div className="absolute inset-0 bg-black/80 backdrop-blur-xl" onClick={onClose} />

      {/* Content */}
      <div className="relative flex flex-1 flex-col items-center justify-center px-6 py-12">
        {/* Close */}
        <button
          type="button"
          onClick={onClose}
          className="absolute top-4 left-1/2 -translate-x-1/2 rounded-full p-2 text-white/80 hover:text-white transition-colors focus-visible:ring-2 focus-visible:ring-amber-500 focus-visible:outline-none"
          aria-label={t("close")}
        >
          <ChevronDownIcon className="size-7" />
        </button>

        {/* Cover Art */}
        <div className="mb-8 w-full max-w-[280px] aspect-square">
          {np.cover_url ? (
            <img
              src={np.cover_url}
              alt={np.album}
              className="size-full rounded-2xl object-cover shadow-2xl"
            />
          ) : (
            <div className="size-full rounded-2xl bg-white/10 flex items-center justify-center shadow-2xl">
              <svg className="size-16 text-white/20" viewBox="0 0 24 24" fill="currentColor">
                <path d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z" />
              </svg>
            </div>
          )}
        </div>

        {/* Track Info */}
        <div className="w-full max-w-[320px] text-center mb-6">
          <h2 className="text-lg font-semibold text-white truncate">{np.title}</h2>
          <p className="text-sm text-white/80 truncate">{np.artist}</p>
          {np.album && <p className="text-xs text-white/40 truncate mt-0.5">{np.album}</p>}
        </div>

        {/* Seek Bar */}
        {np.seekable && np.duration_ms > 0 && (
          <div className="w-full max-w-[320px] mb-6">
            <input
              ref={seekRef}
              type="range"
              min={0}
              max={np.duration_ms}
              value={positionSource}
              aria-valuemin={0}
              aria-valuemax={np.duration_ms}
              aria-valuenow={positionSource}
              aria-valuetext={formatTime(positionSource)}
              onChange={(e) => setLocalPosition(Number(e.target.value))}
              onMouseDown={() => setDragging(true)}
              onTouchStart={() => setDragging(true)}
              onMouseUp={() => { setDragging(false); api.nowPlayingSeek(localPosition); }}
              onTouchEnd={() => { setDragging(false); api.nowPlayingSeek(localPosition); }}
              className="w-full h-1 appearance-none rounded-full bg-white/20 accent-amber-500 cursor-pointer [&::-webkit-slider-thumb]:size-3 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white"
              aria-label={t("seek")}
            />
            <div className="flex justify-between mt-1 text-[10px] text-white/40 font-mono">
              <span>{formatTime(positionSource)}</span>
              <span>{formatTime(np.duration_ms)}</span>
            </div>
          </div>
        )}

        {/* Transport Controls */}
        <div className="flex items-center gap-8 mb-8">
          <button
            type="button"
            onClick={() => api.nowPlayingCommand("previous")}
            disabled={!np.can_prev}
            className="p-3 text-white/80 hover:text-white disabled:text-white/20 transition-colors"
            aria-label={t("previous")}
          >
            <PrevIcon className="size-7" />
          </button>
          <button
            type="button"
            onClick={() => api.nowPlayingCommand("play_pause")}
            className="flex size-16 items-center justify-center rounded-full bg-white text-black hover:bg-white/90 transition-colors focus-visible:ring-2 focus-visible:ring-amber-500 focus-visible:outline-none"
            aria-label={np.playing ? t("pause") : t("play")}
          >
            {np.playing ? <PauseIcon className="size-7" /> : <PlayIcon className="size-7 ml-0.5" />}
          </button>
          <button
            type="button"
            onClick={() => api.nowPlayingCommand("next")}
            disabled={!np.can_next}
            className="p-3 text-white/80 hover:text-white disabled:text-white/20 transition-colors"
            aria-label={t("next")}
          >
            <NextIcon className="size-7" />
          </button>
        </div>

        {/* Volume */}
        <div className="w-full max-w-[280px] flex items-center gap-3">
          <button
            type="button"
            onClick={() => api.setNowPlayingVolume(np.muted ? np.volume : 0)}
            className="text-white/80 hover:text-white transition-colors focus-visible:ring-2 focus-visible:ring-amber-500 focus-visible:outline-none"
            aria-label={np.muted ? t("unmute") : t("mute")}
          >
            <VolumeIcon muted={np.muted} className="size-4" />
          </button>
          <input
            type="range"
            min={0}
            max={100}
            value={volumeSource}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={volumeSource}
            aria-valuetext={`${volumeSource}%`}
            onChange={(e) => setLocalVolume(Number(e.target.value))}
            onMouseDown={() => setVolumeDragging(true)}
            onTouchStart={() => setVolumeDragging(true)}
            onMouseUp={() => { setVolumeDragging(false); api.setNowPlayingVolume(localVolume); }}
            onTouchEnd={() => { setVolumeDragging(false); api.setNowPlayingVolume(localVolume); }}
            className="flex-1 h-1 appearance-none rounded-full bg-white/20 accent-amber-500 cursor-pointer [&::-webkit-slider-thumb]:size-3 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white"
            aria-label={t("volume")}
          />
        </div>

        {/* Source badge */}
        {np.album && (
          <p className="mt-6 text-[10px] text-white/30 uppercase tracking-wider">{t("nowPlaying")}</p>
        )}
      </div>
    </div>
  );
}

// ── MiniPlayer ────────────────────────────────────────────────

export function MiniPlayer({ clientEnabled }: { clientEnabled: boolean }) {
  const t = useTranslations("player");
  const [np, setNp] = useState<NowPlaying | null>(null);
  const [sheetOpen, setSheetOpen] = useState(false);

  const fetchNowPlaying = useCallback(() => {
    api.getNowPlaying().then(setNp).catch(() => {});
  }, []);

  useEffect(() => {
    if (clientEnabled) fetchNowPlaying();
  }, [clientEnabled, fetchNowPlaying]);

  useWebSocket("now_playing", (data) => {
    if (data) setNp(data as NowPlaying);
  });

  if (!clientEnabled || !np || !np.title) return null;

  return (
    <>
      <div
        className="fixed bottom-4 left-4 right-4 z-40 mx-auto max-w-2xl cursor-pointer rounded-2xl border border-white/10 bg-black/60 backdrop-blur-lg px-3 py-2.5 shadow-lg transition-transform active:scale-[0.98]"
        onClick={() => setSheetOpen(true)}
        role="button"
        tabIndex={0}
        aria-label={t("nowPlayingBy", { title: np.title, artist: np.artist })}
        onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setSheetOpen(true); } }}
      >
        <div className="flex items-center gap-3">
          {/* Cover */}
          {np.cover_url ? (
            <img src={np.cover_url} alt="" className="size-10 rounded-lg object-cover" />
          ) : (
            <div className="size-10 rounded-lg bg-white/10 flex items-center justify-center">
              <svg className="size-5 text-white/30" viewBox="0 0 24 24" fill="currentColor">
                <path d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z" />
              </svg>
            </div>
          )}

          {/* Title + Artist */}
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium text-white truncate">{np.title}</p>
            <p className="text-xs text-white/50 truncate">{np.artist}</p>
          </div>

          {/* Play/Pause */}
          <button
            type="button"
            onClick={(e) => { e.stopPropagation(); api.nowPlayingCommand("play_pause"); }}
            className="flex size-9 items-center justify-center rounded-full text-white hover:bg-white/10 transition-colors focus-visible:ring-2 focus-visible:ring-amber-500 focus-visible:outline-none"
            aria-label={np.playing ? t("pause") : t("play")}
          >
            {np.playing ? <PauseIcon className="size-5" /> : <PlayIcon className="size-5 ml-0.5" />}
          </button>
        </div>
      </div>

      <NowPlayingSheet np={np} open={sheetOpen} onClose={() => setSheetOpen(false)} />
    </>
  );
}
