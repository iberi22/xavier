import React, { useRef, useState } from 'react';
import { motion, useDragControls } from 'motion/react';
import { CanvasWidget } from '../types';
import { GripHorizontal, X } from 'lucide-react';

interface DraggableWidgetProps {
  key?: React.Key;
  widget: CanvasWidget;
  onRemove: (id: string) => void;
  onUpdatePosition: (id: string, x: number, y: number) => void;
}

export default function DraggableWidget({ widget, onRemove, onUpdatePosition }: DraggableWidgetProps) {
  const controls = useDragControls();
  const [isDragging, setIsDragging] = useState(false);

  return (
    <motion.div
      drag
      dragControls={controls}
      dragListener={false}
      dragMomentum={false}
      onDragStart={() => setIsDragging(true)}
      onDragEnd={(e, info) => {
        setIsDragging(false);
        onUpdatePosition(widget.id, widget.position.x + info.offset.x, widget.position.y + info.offset.y);
      }}
      initial={{ opacity: 0, scale: 0.8, x: widget.position.x, y: widget.position.y }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.8 }}
      className={`absolute top-0 left-0 z-50 w-64 rounded-xl border backdrop-blur-xl transition-shadow ${isDragging ? 'border-[#39ff14]/50 shadow-[0_0_30px_rgba(57,255,20,0.2)]' : 'border-white/10 shadow-2xl'} bg-[#050505]/80 overflow-hidden pointer-events-auto`}
    >
      <div 
        onPointerDown={(e) => controls.start(e)}
        className="h-8 border-b border-white/5 bg-white/5 flex items-center justify-between px-3 cursor-grab active:cursor-grabbing"
      >
        <div className="flex items-center gap-2 text-white/50 pointer-events-none">
          <GripHorizontal className="w-4 h-4" />
          <span className="text-[10px] uppercase font-mono tracking-widest">{widget.artifact.category}</span>
        </div>
        <button 
          onPointerDown={(e) => e.stopPropagation()} 
          onClick={() => onRemove(widget.id)}
          className="text-white/30 hover:text-red-400 p-0.5 pointer-events-auto transition-colors z-50 rounded bg-white/5 hover:bg-red-400/10"
        >
          <X className="w-3.5 h-3.5" />
        </button>
      </div>
      
      <div className="p-4 pointer-events-auto">
        <h3 className="text-sm font-medium text-white mb-1 leading-tight">{widget.artifact.title}</h3>
        <p className="text-[10px] text-white/40 font-mono mb-3">{widget.artifact.type}</p>
        
        {/* Render artifact fake content based on type (similarly to bookmarks) */}
        <div className="w-full h-16 rounded bg-black/50 border border-white/5 flex items-center justify-center pointer-events-none" style={{ userSelect: 'none' }}>
           {widget.artifact.type === 'Table' && (
             <div className="w-full p-2 flex flex-col gap-1 opacity-60">
               <div className="h-1.5 w-full bg-[#39ff14]/80 rounded" />
               <div className="h-1.5 w-full bg-white/40 rounded" />
               <div className="h-1.5 w-full bg-white/20 rounded" />
             </div>
           )}
           {widget.artifact.type === 'Graph' && (
             <svg className="w-full h-full opacity-60 text-[#39ff14]" viewBox="0 0 100 40" preserveAspectRatio="none">
                <polyline fill="none" stroke="currentColor" strokeWidth="2" points="0,40 20,20 40,30 60,10 80,15 100,5" />
             </svg>
           )}
           {widget.artifact.type === 'Code Snippet' && (
              <div className="w-full p-2 flex flex-col gap-1 opacity-60">
                <div className="h-1.5 w-1/3 bg-[#39ff14] rounded" />
                <div className="h-1.5 w-1/2 bg-white/80 rounded ml-2" />
                <div className="h-1.5 w-1/4 bg-white/60 rounded ml-2" />
              </div>
           )}
           {widget.artifact.type === 'Data Card' && (
              <div className="flex gap-2 p-2 w-full opacity-80">
                 <div className="w-6 h-6 rounded-full border-2 border-[#39ff14]/80" />
                 <div className="flex-1 flex flex-col justify-center gap-1">
                    <div className="h-1.5 w-3/4 bg-white/80 rounded" />
                    <div className="h-1.5 w-1/2 bg-[#39ff14]/60 rounded" />
                 </div>
              </div>
           )}
        </div>
      </div>
    </motion.div>
  );
}
