import { useState } from 'react';
import { Mic, Send, BrainCircuit } from 'lucide-react';

interface InputAreaProps {
  onSendMessage: (text: string) => void;
  onOpenConfig: () => void;
}

export default function InputArea({ onSendMessage, onOpenConfig }: InputAreaProps) {
  const [inputText, setInputText] = useState('');
  const [isRecording, setIsRecording] = useState(false);
  const [isTranscribing, setIsTranscribing] = useState(false);

  const handleMicClick = () => {
    if (isRecording) {
      setIsRecording(false);
      setIsTranscribing(true);
      // Simulate transcription delay
      setTimeout(() => {
        setIsTranscribing(false);
        setInputText((prev) => prev + (prev ? ' ' : '') + 'Audio transcript processed.');
      }, 2000);
    } else {
      setIsRecording(true);
      setIsTranscribing(false);
    }
  };

  const handleSend = () => {
    if (!inputText.trim()) return;
    onSendMessage(inputText);
    setInputText('');
  };

  return (
    <div className="absolute bottom-8 left-1/2 -translate-x-1/2 w-full max-w-2xl px-4 pointer-events-auto z-10">
      <div className="glass rounded-[24px] p-2 flex items-center gap-2 relative overflow-hidden transition-all duration-300 focus-within:shadow-[0_0_20px_rgba(57,255,20,0.15)] focus-within:border-white/20">
        
        {/* Animated background when recording */}
        {isRecording && (
          <div className="absolute inset-0 bg-[#39ff14]/5 animate-pulse" />
        )}

        <button
          onClick={onOpenConfig}
          className="relative z-10 w-12 h-12 flex items-center justify-center rounded-full transition-all duration-300 hover:bg-white/5 text-[#39ff14] hover:scale-105 active:scale-95 group"
          title="Open Control Node"
        >
          <div className="absolute inset-1 rounded-full border border-transparent group-hover:border-[#39ff14]/40 group-hover:shadow-[inset_0_0_10px_rgba(57,255,20,0.2)] transition-all duration-300" />
          <BrainCircuit className="w-6 h-6 group-hover:drop-shadow-[0_0_10px_rgba(57,255,20,0.8)] transition-all duration-300" strokeWidth={1.5} />
        </button>

        <div className="w-px h-8 bg-white/10 relative z-10 mx-1" />

        <button 
          onClick={handleMicClick}
          className={`relative z-10 w-12 h-12 flex items-center justify-center rounded-full transition-all duration-300 hover:bg-white/5 ${
            isRecording ? 'text-[#39ff14]' : 'text-white/60 hover:text-[#39ff14]'
          }`}
          title={isRecording ? "Stop recording" : "Record audio"}
        >
          {isRecording ? (
            <div className="flex gap-[3px] items-center justify-center h-full">
               <div className="w-[3px] bg-[#39ff14] rounded-full animate-[audioBar_1s_ease-in-out_infinite_0ms]" style={{height: '12px'}} />
               <div className="w-[3px] bg-[#39ff14] rounded-full animate-[audioBar_1s_ease-in-out_infinite_100ms]" style={{height: '24px'}} />
               <div className="w-[3px] bg-[#39ff14] rounded-full animate-[audioBar_1s_ease-in-out_infinite_200ms]" style={{height: '16px'}} />
               <div className="w-[3px] bg-[#39ff14] rounded-full animate-[audioBar_1s_ease-in-out_infinite_300ms]" style={{height: '20px'}} />
            </div>
          ) : (
            <Mic className="w-5 h-5" />
          )}
        </button>

        <div className="flex-1 relative z-10 flex flex-col justify-center min-h-[48px]">
          {isTranscribing ? (
             <div className="flex items-center gap-2 px-2 text-[#39ff14]/80 text-sm italic font-medium w-full animate-pulse">
               Transcribing
               <span className="flex gap-1">
                 <span className="w-1 h-1 bg-[#39ff14] rounded-full animate-bounce" style={{animationDelay: '-0.3s'}}></span>
                 <span className="w-1 h-1 bg-[#39ff14] rounded-full animate-bounce" style={{animationDelay: '-0.15s'}}></span>
                 <span className="w-1 h-1 bg-[#39ff14] rounded-full animate-bounce"></span>
               </span>
             </div>
          ) : (
            <input 
              type="text" 
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleSend()}
              placeholder={isRecording ? "Listening..." : "Initialize command sequence..."}
              className="w-full bg-transparent border-none outline-none text-white px-2 placeholder:text-white/30 text-sm font-medium"
              disabled={isRecording}
            />
          )}
        </div>

        <button 
          onClick={handleSend}
          disabled={!inputText.trim() && !isTranscribing}
          className={`relative z-10 w-12 h-12 flex items-center justify-center rounded-full transition-all duration-300 ${
            inputText.trim() ? 'bg-[#39ff14] text-[#050505] hover:brightness-110 shadow-[0_0_15px_rgba(57,255,20,0.3)]' : 'bg-white/5 text-white/30 cursor-not-allowed'
          }`}
          title="Send command"
        >
          <Send className="w-5 h-5 ml-1" />
        </button>
      </div>
    </div>
  );
}
