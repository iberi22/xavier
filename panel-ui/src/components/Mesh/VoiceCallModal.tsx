import {
  AlertCircle,
  Mic,
  MicOff,
  Phone,
  PhoneOff,
  ShieldCheck,
  Volume2,
  VolumeX,
  Wifi,
  WifiOff,
} from "lucide-react";
import React, { useCallback, useEffect, useState } from "react";

export type CallDirection = "incoming" | "outgoing";
export type WebRTCConnectionState =
  | "connecting"
  | "connected"
  | "disconnected"
  | "failed";

export interface VoiceCallPeer {
  node_id: string;
  alias?: string;
  avatar_url?: string;
}

export interface VoiceCallModalProps {
  isOpen: boolean;
  peer: VoiceCallPeer;
  direction: CallDirection;
  connectionState?: WebRTCConnectionState;
  onAccept?: () => void;
  onDecline?: () => void;
  onEndCall: () => void;
  onToggleMute?: (isMuted: boolean) => void;
  onToggleSpeaker?: (isSpeakerMuted: boolean) => void;
}

export const VoiceCallModal: React.FC<VoiceCallModalProps> = ({
  isOpen,
  peer,
  direction,
  connectionState = "connecting",
  onAccept,
  onDecline,
  onEndCall,
  onToggleMute,
  onToggleSpeaker,
}) => {
  const [isMuted, setIsMuted] = useState(false);
  const [isSpeakerMuted, setIsSpeakerMuted] = useState(false);
  const [micError, setMicError] = useState<string | null>(null);
  const [isCallAccepted, setIsCallAccepted] = useState(
    direction === "outgoing",
  );

  // Request microphone access and handle potential permission errors gracefully
  const requestMicrophoneAccess = useCallback(async () => {
    setMicError(null);
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
      setMicError(
        "Audio recording is not supported in this browser environment.",
      );
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: true,
      });
      // Stop temporary track after verifying permission
      stream.getTracks().forEach((track) => track.stop());
    } catch (err) {
      console.warn("Microphone access permission error:", err);
      if (err instanceof DOMException && err.name === "NotAllowedError") {
        setMicError(
          "Microphone permission denied. Please enable mic access to talk.",
        );
      } else if (
        err instanceof DOMException &&
        err.name === "NotFoundError"
      ) {
        setMicError(
          "No microphone hardware found on this device.",
        );
      } else {
        setMicError(
          "Failed to access microphone. Please check your system audio settings.",
        );
      }
    }
  }, []);

  useEffect(() => {
    if (isOpen && (direction === "outgoing" || isCallAccepted)) {
      requestMicrophoneAccess();
    }
  }, [isOpen, direction, isCallAccepted, requestMicrophoneAccess]);

  if (!isOpen) return null;

  const handleAcceptCall = () => {
    setIsCallAccepted(true);
    if (onAccept) {
      onAccept();
    }
  };

  const handleToggleMute = () => {
    const newMuteState = !isMuted;
    setIsMuted(newMuteState);
    if (onToggleMute) {
      onToggleMute(newMuteState);
    }
  };

  const handleToggleSpeaker = () => {
    const newSpeakerState = !isSpeakerMuted;
    setIsSpeakerMuted(newSpeakerState);
    if (onToggleSpeaker) {
      onToggleSpeaker(newSpeakerState);
    }
  };

  const peerInitials =
    peer.alias && peer.alias.length > 0
      ? peer.alias.substring(0, 2).toUpperCase()
      : peer.node_id.substring(0, 2).toUpperCase();

  const currentStatusLabel = !isCallAccepted
    ? "Incoming P2P Audio Call..."
    : connectionState === "connecting"
      ? "Connecting WebRTC..."
      : connectionState === "connected"
        ? "Encrypted P2P Voice Active"
        : connectionState === "failed"
          ? "Connection Failed"
          : "Disconnected";

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="voice-call-title"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-md p-4 animate-in fade-in duration-200"
    >
      <div className="w-full max-w-sm rounded-3xl bg-neutral-900 border border-neutral-800 shadow-2xl p-6 flex flex-col items-center text-center relative overflow-hidden">
        {/* Background ambient glow based on connection state */}
        <div
          className={`absolute -top-24 -left-24 w-48 h-48 rounded-full blur-3xl opacity-20 pointer-events-none ${
            connectionState === "connected" && isCallAccepted
              ? "bg-emerald-500"
              : connectionState === "failed"
                ? "bg-red-500"
                : "bg-amber-500"
          }`}
        />

        {/* Header: Network Security Status Indicator */}
        <div className="w-full flex items-center justify-between text-xs mb-6">
          <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-neutral-800/80 border border-neutral-700/50 text-neutral-300">
            {connectionState === "connected" && isCallAccepted ? (
              <>
                <ShieldCheck className="w-3.5 h-3.5 text-emerald-400" />
                <span className="font-mono text-[11px] text-emerald-400">
                  Encrypted P2P
                </span>
              </>
            ) : connectionState === "failed" || connectionState === "disconnected" ? (
              <>
                <WifiOff className="w-3.5 h-3.5 text-red-400" />
                <span className="font-mono text-[11px] text-red-400">
                  Disconnected
                </span>
              </>
            ) : (
              <>
                <Wifi className="w-3.5 h-3.5 text-amber-400 animate-pulse" />
                <span className="font-mono text-[11px] text-amber-400">
                  Connecting...
                </span>
              </>
            )}
          </div>
          <span className="font-mono text-[10px] text-neutral-500 tracking-wider uppercase">
            {direction === "incoming" && !isCallAccepted ? "INCOMING" : "P2P VOICE"}
          </span>
        </div>

        {/* Peer Avatar & Details */}
        <div className="relative mb-4">
          {peer.avatar_url ? (
            <img
              src={peer.avatar_url}
              alt={peer.alias || peer.node_id}
              className="w-24 h-24 rounded-full object-cover border-2 border-neutral-700 shadow-lg"
            />
          ) : (
            <div className="w-24 h-24 rounded-full bg-gradient-to-br from-neutral-700 to-neutral-800 border-2 border-neutral-600 flex items-center justify-center text-2xl font-bold text-neutral-200 shadow-lg">
              {peerInitials}
            </div>
          )}
          {isCallAccepted && connectionState === "connected" && (
            <div className="absolute bottom-0 right-0 w-6 h-6 rounded-full bg-emerald-500 border-2 border-neutral-900 flex items-center justify-center shadow">
              <span className="w-2 h-2 rounded-full bg-white animate-ping" />
            </div>
          )}
        </div>

        <h2 id="voice-call-title" className="text-xl font-semibold text-neutral-100 mb-1">
          {peer.alias || "Xavier Node"}
        </h2>
        <p className="font-mono text-xs text-neutral-400 mb-4 truncate max-w-[240px]" title={peer.node_id}>
          {peer.node_id}
        </p>

        {/* Connection Status Subtitle */}
        <p className="text-xs text-neutral-400 font-medium mb-4">
          {currentStatusLabel}
        </p>

        {/* Microphone Access Error Notice (Anti-Hallucination Guard) */}
        {micError && (
          <div className="w-full mb-4 p-3 rounded-xl bg-red-950/60 border border-red-800/60 text-red-300 text-xs text-left flex items-start gap-2.5">
            <AlertCircle className="w-4 h-4 text-red-400 shrink-0 mt-0.5" />
            <div className="flex-1">
              <p className="font-medium text-red-200">Microphone Issue</p>
              <p className="text-[11px] text-red-300/90 mt-0.5">{micError}</p>
            </div>
          </div>
        )}

        {/* Action Controls */}
        <div className="w-full flex items-center justify-center gap-4 mt-2">
          {!isCallAccepted && direction === "incoming" ? (
            <>
              {/* Incoming Call: Accept / Decline buttons */}
              <button
                type="button"
                onClick={onDecline || onEndCall}
                aria-label="Decline voice call"
                className="w-14 h-14 rounded-full bg-red-600 hover:bg-red-500 text-white flex items-center justify-center transition-transform hover:scale-105 active:scale-95 shadow-lg focus:outline-none focus:ring-2 focus:ring-red-400 focus:ring-offset-2 focus:ring-offset-neutral-900"
              >
                <PhoneOff className="w-6 h-6" />
              </button>
              <button
                type="button"
                onClick={handleAcceptCall}
                aria-label="Accept voice call"
                className="w-14 h-14 rounded-full bg-emerald-600 hover:bg-emerald-500 text-white flex items-center justify-center transition-transform hover:scale-105 active:scale-95 shadow-lg focus:outline-none focus:ring-2 focus:ring-emerald-400 focus:ring-offset-2 focus:ring-offset-neutral-900"
              >
                <Phone className="w-6 h-6 animate-bounce" />
              </button>
            </>
          ) : (
            <>
              {/* Active / Outgoing Call Controls: Mute, Speaker, End Call */}
              <button
                type="button"
                onClick={handleToggleMute}
                aria-label={isMuted ? "Unmute microphone" : "Mute microphone"}
                className={`w-12 h-12 rounded-full flex items-center justify-center transition-all ${
                  isMuted
                    ? "bg-amber-500/20 text-amber-400 border border-amber-500/40 hover:bg-amber-500/30"
                    : "bg-neutral-800 text-neutral-200 border border-neutral-700 hover:bg-neutral-700"
                } focus:outline-none focus:ring-2 focus:ring-neutral-400 focus:ring-offset-2 focus:ring-offset-neutral-900`}
              >
                {isMuted ? (
                  <MicOff className="w-5 h-5" aria-hidden="true" />
                ) : (
                  <Mic className="w-5 h-5" aria-hidden="true" />
                )}
              </button>

              <button
                type="button"
                onClick={onEndCall}
                aria-label="End call"
                className="w-14 h-14 rounded-full bg-red-600 hover:bg-red-500 text-white flex items-center justify-center transition-transform hover:scale-105 active:scale-95 shadow-lg focus:outline-none focus:ring-2 focus:ring-red-400 focus:ring-offset-2 focus:ring-offset-neutral-900"
              >
                <PhoneOff className="w-6 h-6" aria-hidden="true" />
              </button>

              <button
                type="button"
                onClick={handleToggleSpeaker}
                aria-label={isSpeakerMuted ? "Turn on speaker" : "Mute speaker"}
                className={`w-12 h-12 rounded-full flex items-center justify-center transition-all ${
                  isSpeakerMuted
                    ? "bg-amber-500/20 text-amber-400 border border-amber-500/40 hover:bg-amber-500/30"
                    : "bg-neutral-800 text-neutral-200 border border-neutral-700 hover:bg-neutral-700"
                } focus:outline-none focus:ring-2 focus:ring-neutral-400 focus:ring-offset-2 focus:ring-offset-neutral-900`}
              >
                {isSpeakerMuted ? (
                  <VolumeX className="w-5 h-5" aria-hidden="true" />
                ) : (
                  <Volume2 className="w-5 h-5" aria-hidden="true" />
                )}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
};

export default VoiceCallModal;
