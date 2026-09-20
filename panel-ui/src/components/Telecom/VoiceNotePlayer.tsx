import {
  FastForward,
  Lock,
  Pause,
  Play,
  Volume2,
  VolumeX,
} from "lucide-react";
import React, {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";

export interface VoiceNotePlayerProps {
  src?: string;
  duration?: number;
  waveform?: number[];
  title?: string;
  author?: string;
  timestamp?: string;
  isEncrypted?: boolean;
  onPlayStateChange?: (isPlaying: boolean) => void;
  onEnded?: () => void;
  autoPlay?: boolean;
  className?: string;
}

// Generate deterministic fallback waveform bars if none provided
const generateDefaultWaveform = (barCount = 36): number[] => {
  const bars: number[] = [];
  for (let i = 0; i < barCount; i++) {
    const val = Math.abs(
      Math.sin(i * 0.45) * 0.6 + Math.cos(i * 0.85) * 0.35 + 0.2,
    );
    bars.push(Math.min(1, Math.max(0.15, val)));
  }
  return bars;
};

export const VoiceNotePlayer: React.FC<VoiceNotePlayerProps> = ({
  src,
  duration: initialDuration = 12,
  waveform: providedWaveform,
  title = "P2P Voice Note",
  author,
  timestamp,
  isEncrypted = true,
  onPlayStateChange,
  onEnded,
  autoPlay = false,
  className = "",
}) => {
  const [isPlaying, setIsPlaying] = useState(autoPlay);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(initialDuration);
  const [playbackRate, setPlaybackRate] = useState<1 | 1.5 | 2>(1);
  const [isMuted, setIsMuted] = useState(false);
  const [isHovered, setIsHovered] = useState(false);

  const audioRef = useRef<HTMLAudioElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const isDraggingRef = useRef(false);
  const canvasId = useId();

  const waveformBars = useRef<number[]>(
    providedWaveform && providedWaveform.length > 0
      ? providedWaveform
      : generateDefaultWaveform(36),
  ).current;

  // Toggle Play / Pause state
  const togglePlayback = useCallback(() => {
    setIsPlaying((prev) => {
      const nextState = !prev;
      if (onPlayStateChange) {
        onPlayStateChange(nextState);
      }
      return nextState;
    });
  }, [onPlayStateChange]);

  // Cycle speed multiplier (1x -> 1.5x -> 2x -> 1x)
  const toggleSpeed = useCallback(() => {
    setPlaybackRate((prevRate) => {
      if (prevRate === 1) return 1.5;
      if (prevRate === 1.5) return 2;
      return 1;
    });
  }, []);

  // Toggle audio mute
  const toggleMute = useCallback(() => {
    setIsMuted((prev) => !prev);
  }, []);

  // Seek playback position to target ratio (0..1)
  const seekToRatio = useCallback(
    (ratio: number) => {
      const clamped = Math.max(0, Math.min(1, ratio));
      const targetTime = clamped * (duration || 1);
      setCurrentTime(targetTime);

      if (audioRef.current && Number.isFinite(targetTime)) {
        try {
          audioRef.current.currentTime = targetTime;
        } catch {
          // Fallback if audio element isn't ready or synthetic
        }
      }
    },
    [duration],
  );

  // Synchronize audio element playback rate and muted status
  useEffect(() => {
    if (audioRef.current) {
      audioRef.current.playbackRate = playbackRate;
      audioRef.current.muted = isMuted;
    }
  }, [playbackRate, isMuted]);

  // Play / Pause audio element effect with fallback simulation timer
  useEffect(() => {
    let animationFrameId: number;
    let timerId: NodeJS.Timeout;

    if (src && audioRef.current) {
      if (isPlaying) {
        audioRef.current.play().catch(() => {
          // If browser prevents autoplay or audio resource fails, keep simulated playback active
        });
      } else {
        audioRef.current.pause();
      }
    } else if (isPlaying) {
      // Fallback timer simulation when no src audio resource is attached
      const intervalMs = 100 / playbackRate;
      timerId = setInterval(() => {
        setCurrentTime((prev) => {
          const next = prev + 0.1 * playbackRate;
          if (next >= duration) {
            setIsPlaying(false);
            if (onEnded) onEnded();
            return 0;
          }
          return next;
        });
      }, intervalMs);
    }

    return () => {
      if (animationFrameId) cancelAnimationFrame(animationFrameId);
      if (timerId) clearInterval(timerId);
    };
  }, [isPlaying, src, playbackRate, duration, onEnded]);

  // Draw waveform onto HTML5 canvas
  const drawWaveform = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const width = canvas.width;
    const height = canvas.height;
    ctx.clearRect(0, 0, width, height);

    const progressRatio = duration > 0 ? currentTime / duration : 0;
    const barCount = waveformBars.length;
    const gap = 3;
    const totalGap = gap * (barCount - 1);
    const barWidth = Math.max(2, (width - totalGap) / barCount);
    const borderRadius = 2;

    waveformBars.forEach((barHeightNorm, index) => {
      const x = index * (barWidth + gap);
      const barRatio = (index + 0.5) / barCount;
      const isPlayed = barRatio <= progressRatio;

      const barH = Math.max(4, barHeightNorm * (height - 6));
      const y = (height - barH) / 2;

      // Color selection
      if (isPlayed) {
        ctx.fillStyle = "#39ff14"; // Active neon accent
      } else {
        ctx.fillStyle = isHovered ? "rgba(255, 255, 255, 0.35)" : "rgba(255, 255, 255, 0.2)";
      }

      // Draw rounded rectangle bar
      ctx.beginPath();
      if (ctx.roundRect) {
        ctx.roundRect(x, y, barWidth, barH, borderRadius);
      } else {
        ctx.rect(x, y, barWidth, barH);
      }
      ctx.fill();
    });
  }, [currentTime, duration, waveformBars, isHovered]);

  // Re-draw waveform on state or canvas resize changes
  useEffect(() => {
    drawWaveform();
  }, [drawWaveform]);

  // Handle canvas sizing / DPR scaling
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const updateCanvasSize = () => {
      const rect = canvas.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        const dpr = window.devicePixelRatio || 1;
        canvas.width = rect.width * dpr;
        canvas.height = rect.height * dpr;
        const ctx = canvas.getContext("2d");
        if (ctx) ctx.scale(dpr, dpr);
        drawWaveform();
      }
    };

    updateCanvasSize();
    const resizeObserver = new ResizeObserver(() => updateCanvasSize());
    resizeObserver.observe(canvas);

    return () => {
      resizeObserver.disconnect();
    };
  }, [drawWaveform]);

  // Canvas click & drag interactions for seeking
  const handleCanvasPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    isDraggingRef.current = true;
    e.currentTarget.setPointerCapture(e.pointerId);
    const rect = e.currentTarget.getBoundingClientRect();
    const clickX = e.clientX - rect.left;
    seekToRatio(clickX / rect.width);
  };

  const handleCanvasPointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (!isDraggingRef.current) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const clickX = e.clientX - rect.left;
    seekToRatio(clickX / rect.width);
  };

  const handleCanvasPointerUp = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (isDraggingRef.current) {
      isDraggingRef.current = false;
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  };

  // Format time in MM:SS
  const formatTime = (secs: number) => {
    if (!Number.isFinite(secs) || secs < 0) return "00:00";
    const minutes = Math.floor(secs / 60);
    const seconds = Math.floor(secs % 60);
    return `${minutes.toString().padStart(2, "0")}:${seconds
      .toString()
      .padStart(2, "0")}`;
  };

  return (
    <div
      className={`group relative flex flex-col gap-2 p-3.5 rounded-2xl bg-slate-900/90 border border-white/10 text-white shadow-lg backdrop-blur-md transition-all hover:border-[#39ff14]/30 ${className}`}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
    >
      {/* Hidden audio element for standard HTML audio playback */}
      {src && (
        <audio
          ref={audioRef}
          src={src}
          preload="metadata"
          onLoadedMetadata={() => {
            if (audioRef.current && Number.isFinite(audioRef.current.duration)) {
              setDuration(audioRef.current.duration);
            }
          }}
          onTimeUpdate={() => {
            if (audioRef.current) {
              setCurrentTime(audioRef.current.currentTime);
            }
          }}
          onEnded={() => {
            setIsPlaying(false);
            setCurrentTime(0);
            if (onEnded) onEnded();
          }}
        />
      )}

      {/* Top Header Row: Author/Title + Security Tag */}
      <div className="flex items-center justify-between text-xs">
        <div className="flex items-center gap-2 truncate">
          <span className="font-semibold text-white truncate max-w-[180px]">
            {title}
          </span>
          {author && (
            <span className="text-[10px] text-white/50 font-mono truncate">
              · {author}
            </span>
          )}
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {isEncrypted && (
            <span className="flex items-center gap-1 text-[10px] font-mono text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20">
              <Lock className="w-2.5 h-2.5" aria-hidden="true" />
              <span>Encrypted</span>
            </span>
          )}
          {timestamp && (
            <span className="text-[10px] text-white/40 font-mono">
              {timestamp}
            </span>
          )}
        </div>
      </div>

      {/* Main Controls & Waveform Row */}
      <div className="flex items-center gap-3 mt-1">
        {/* Play/Pause Button */}
        <button
          type="button"
          onClick={togglePlayback}
          aria-label={isPlaying ? "Pause voice note" : "Play voice note"}
          className="w-10 h-10 rounded-xl bg-[#39ff14]/10 border border-[#39ff14]/30 text-[#39ff14] flex items-center justify-center shrink-0 hover:bg-[#39ff14]/20 active:scale-95 transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]"
        >
          {isPlaying ? (
            <Pause className="w-5 h-5 fill-[#39ff14]" aria-hidden="true" />
          ) : (
            <Play className="w-5 h-5 fill-[#39ff14] ml-0.5" aria-hidden="true" />
          )}
        </button>

        {/* Canvas Waveform Preview Container */}
        <div className="relative flex-1 h-10 flex items-center justify-center bg-black/40 rounded-xl border border-white/5 px-2 overflow-hidden cursor-pointer select-none">
          <canvas
            ref={canvasRef}
            id={canvasId}
            role="slider"
            aria-label="Voice note playback progress"
            aria-valuemin={0}
            aria-valuemax={Math.round(duration)}
            aria-valuenow={Math.round(currentTime)}
            aria-valuetext={`${formatTime(currentTime)} of ${formatTime(duration)}`}
            tabIndex={0}
            onPointerDown={handleCanvasPointerDown}
            onPointerMove={handleCanvasPointerMove}
            onPointerUp={handleCanvasPointerUp}
            onKeyDown={(e) => {
              if (e.key === "ArrowLeft") {
                seekToRatio((currentTime - 2) / duration);
              } else if (e.key === "ArrowRight") {
                seekToRatio((currentTime + 2) / duration);
              } else if (e.key === " " || e.key === "Enter") {
                e.preventDefault();
                togglePlayback();
              }
            }}
            className="w-full h-8 touch-none focus:outline-none focus-visible:ring-1 focus-visible:ring-[#39ff14]"
          />
        </div>

        {/* Speed Multiplier Button */}
        <button
          type="button"
          onClick={toggleSpeed}
          aria-label={`Playback speed: ${playbackRate}x`}
          className="px-2.5 py-2 rounded-xl bg-white/5 hover:bg-white/10 border border-white/10 text-white/80 hover:text-white text-xs font-mono shrink-0 transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]"
        >
          {playbackRate}x
        </button>

        {/* Mute Button */}
        <button
          type="button"
          onClick={toggleMute}
          aria-label={isMuted ? "Unmute audio" : "Mute audio"}
          className="p-2 rounded-xl bg-white/5 hover:bg-white/10 border border-white/10 text-white/70 hover:text-white shrink-0 transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]"
        >
          {isMuted ? (
            <VolumeX className="w-4 h-4 text-amber-400" aria-hidden="true" />
          ) : (
            <Volume2 className="w-4 h-4" aria-hidden="true" />
          )}
        </button>
      </div>

      {/* Progress Indicator Footer */}
      <div className="flex items-center justify-between text-[10px] font-mono text-white/40 px-1">
        <span>{formatTime(currentTime)}</span>
        <div className="flex items-center gap-1">
          <FastForward className="w-2.5 h-2.5 text-[#39ff14]/60" aria-hidden="true" />
          <span>OPUS / P2P Audio</span>
        </div>
        <span>{formatTime(duration)}</span>
      </div>
    </div>
  );
};

export default VoiceNotePlayer;
