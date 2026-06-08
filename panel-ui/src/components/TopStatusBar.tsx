import { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import { Database, Activity, Wifi, Bot, Zap, Bell, ShieldCheck, Key, MessageCircle, Hash, Users, Send, MessageSquare, Settings } from 'lucide-react';

interface TopStatusBarProps {
  isModalOpen?: boolean;
}

export default function TopStatusBar({ isModalOpen = false }: TopStatusBarProps) {
  const [time, setTime] = useState(new Date());
  const [notifications] = useState(12); // Example notification count
  const [showConfig, setShowConfig] = useState(false);
  const [modules, setModules] = useState({
    time: true,
    channels: true,
    resources: true,
    security: true,
    sync: true,
    ai: true,
    notifications: true
  });

  const toggleModule = (key: keyof typeof modules) => {
    setModules(prev => ({ ...prev, [key]: !prev[key] }));
  };

  useEffect(() => {
    const interval = setInterval(() => {
      setTime(new Date());
    }, 1000);
    return () => clearInterval(interval);
  }, []);

  const spring = { type: "spring" as const, stiffness: 200, damping: 25 };

  return (
    <div className="absolute inset-0 z-[60] pointer-events-none overflow-hidden">
       {/* Left Group */}
       <motion.div 
         layout
         transition={spring}
         className={`flex gap-2 pointer-events-auto ${isModalOpen ? 'absolute left-2 lg:left-4 top-1/2 -translate-y-1/2 flex-col items-start z-[60]' : 'absolute left-4 md:left-6 top-6 flex-row items-start'}`}
       >
          {/* Time & Date Pill */}
          {modules.time && (
            <motion.div layout transition={spring} className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-3 py-1 flex items-center gap-2 h-7 text-white/80 shrink-0">
               <span className="font-mono text-[10px] hidden md:inline-block">{time.toLocaleDateString(undefined, {month: 'numeric', day: 'numeric'})}</span>
               <div className="w-px h-2.5 bg-white/20 hidden md:block" />
               <span className="font-mono text-[10px] min-w-[50px] text-center">{time.toLocaleTimeString(undefined, {hour: '2-digit', minute:'2-digit'})}</span>
            </motion.div>
          )}

          {/* System Resources Pill */}
          {modules.resources && (
            <motion.div layout transition={spring} className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-3 py-1 flex items-center gap-3 h-7 shrink-0 hidden lg:flex">
               <div className="flex items-center gap-1 text-[10px] text-white/70" title="Memory Usage: 16.4GB / 32.0GB">
                  <Database className="w-3 h-3 text-blue-400" />
                  <span className="font-mono">16G</span>
               </div>
               <div className="flex items-center gap-1 text-[10px] text-white/70" title="Average CPU Usage: 14%">
                  <Activity className="w-3 h-3 text-red-400" />
                  <span className="font-mono">14%</span>
               </div>
               <div className="flex items-center gap-1 text-[10px] text-[#39ff14]" title="GPU Status: ON">
                  <Zap className="w-3 h-3 fill-[#39ff14]/20" />
               </div>
            </motion.div>
          )}

          {/* Communication Channels */}
          {modules.channels && (
            <motion.div layout transition={spring} className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-1.5 h-7 shrink-0 hidden md:flex">
               <span title="Discord"><MessageCircle className="w-3 h-3 text-indigo-400 hover:text-indigo-300 cursor-pointer transition-colors" /></span>
               <span title="Slack"><Hash className="w-3 h-3 text-amber-400 hover:text-amber-300 cursor-pointer transition-colors" /></span>
               <span title="Teams"><Users className="w-3 h-3 text-purple-400 hover:text-purple-300 cursor-pointer transition-colors" /></span>
               <span title="Telegram"><Send className="w-3 h-3 text-blue-400 hover:text-blue-300 cursor-pointer transition-colors" /></span>
               <span title="WhatsApp"><MessageSquare className="w-3 h-3 text-green-400 hover:text-green-300 cursor-pointer transition-colors" /></span>
            </motion.div>
          )}
       </motion.div>

       {/* Center - Identity */}
       <motion.div 
         layout
         transition={spring}
         className={`absolute pointer-events-auto top-6 left-1/2 -translate-x-1/2 z-[60]`}
       >
          <div className="relative group">
            <motion.div layout transition={spring} className="bg-[#0a0a0a]/90 backdrop-blur-md border border-[#39ff14]/30 shadow-[0_0_15px_rgba(57,255,20,0.15)] rounded-full px-3 py-1 flex items-center justify-center cursor-default shrink-0 min-h-[28px]">
               <span className="text-[#39ff14] font-mono tracking-widest text-[8px] uppercase font-bold">Xavier Beta</span>
            </motion.div>
            
            {/* Gear Button */}
            <button 
              onClick={() => setShowConfig(!showConfig)}
              className="absolute -right-7 top-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100 transition-opacity text-white/30 hover:text-[#39ff14] p-1.5 outline-none"
              title="Configure Status Bar"
            >
              <Settings className="w-3.5 h-3.5 hover:animate-[spin_4s_linear_infinite]" />
            </button>

            {/* Config Popover */}
            <AnimatePresence>
              {showConfig && (
                <motion.div 
                  initial={{ opacity: 0, y: 10, scale: 0.95 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={{ opacity: 0, y: 10, scale: 0.95 }}
                  className="absolute top-full mt-3 left-1/2 -translate-x-1/2 w-48 bg-[#0a0a0a]/95 backdrop-blur-xl border border-white/10 rounded-xl p-3 shadow-2xl flex flex-col gap-2 z-[60]"
                >
                   <h3 className="text-[10px] uppercase tracking-widest text-white/40 mb-1 px-1">Modules</h3>
                   {Object.entries({
                     time: 'Time & Date',
                     resources: 'System Resources',
                     channels: 'Communication',
                     security: 'Security & Proxy',
                     sync: 'Node Sync',
                     ai: 'AI Providers',
                     notifications: 'Notifications'
                   }).map(([key, label]) => (
                     <button 
                       key={key}
                       onClick={() => toggleModule(key as keyof typeof modules)}
                       className="flex items-center justify-between px-2 py-1.5 hover:bg-white/5 rounded-lg transition-colors group/btn outline-none"
                     >
                        <span className="text-xs text-white/80 font-mono">{label}</span>
                        <div className={`w-3 h-3 rounded-sm border flex items-center justify-center transition-colors ${modules[key as keyof typeof modules] ? 'bg-[#39ff14]/20 border-[#39ff14]/50' : 'border-white/20 group-hover/btn:border-white/40'}`}>
                           {modules[key as keyof typeof modules] && <div className="w-1.5 h-1.5 bg-[#39ff14] rounded-[1px]" />}
                        </div>
                     </button>
                   ))}
                </motion.div>
              )}
            </AnimatePresence>
          </div>
       </motion.div>

       {/* Right Group */}
       <motion.div 
         layout
         transition={spring}
         className={`flex gap-2 pointer-events-auto ${isModalOpen ? 'absolute right-2 lg:right-4 top-1/2 -translate-y-1/2 flex-col items-end z-[60]' : 'absolute right-4 md:right-6 top-6 flex-row items-start'}`}
       >
          {/* Security & Proxy */}
          {modules.security && (
            <motion.div layout transition={spring} className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-2 h-7 shrink-0 lg:flex">
               <div className="flex items-center gap-1" title="TPM HW Encryption Active">
                 <ShieldCheck className="w-3 h-3 text-emerald-400" />
               </div>
               <div className="w-px h-2.5 bg-white/20" />
               <div className="flex items-center gap-1" title="API Token Proxy">
                 <Key className="w-3 h-3 text-yellow-500" />
               </div>
            </motion.div>
          )}

          {/* Node Sync Pill */}
          {modules.sync && (
            <motion.div layout transition={spring} className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-1.5 h-7 shrink-0 sm:flex">
               <Wifi className="w-3 h-3 text-cyan-400" />
               <span className="font-mono text-[9px] text-cyan-400 uppercase tracking-wide hidden md:inline-block">4</span>
               <div className="w-8 h-0.5 bg-black/50 rounded-full overflow-hidden border border-white/5 mx-0.5 hidden xl:block">
                  <div className="h-full bg-cyan-400 w-[98%] shadow-[0_0_8px_rgba(34,211,238,0.8)]" />
               </div>
               <span className="font-mono text-[9px] text-cyan-400 font-bold">98%</span>
            </motion.div>
          )}

          {/* AI Providers Pill */}
          {modules.ai && (
            <motion.div layout transition={spring} className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-1.5 h-7 shrink-0">
               <Bot className="w-3 h-3 text-purple-400" />
               <div className="flex items-center gap-1">
                  <span className="hidden md:inline-block text-[8px] bg-blue-500/20 text-blue-300 border border-blue-500/30 px-1 py-px rounded uppercase tracking-wider font-mono">GEM</span>
                  <span className="hidden md:inline-block text-[8px] bg-emerald-500/20 text-emerald-300 border border-emerald-500/30 px-1 py-px rounded uppercase tracking-wider font-mono">OAI</span>
               </div>
            </motion.div>
          )}

          {/* Notifications */}
          {modules.notifications && (
            <motion.button layout transition={spring} className={`bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-2 hover:bg-white/5 transition-colors flex items-center justify-center h-7 shrink-0 ${notifications === 0 ? 'opacity-50' : ''}`} title="Notifications">
               <div className="relative flex items-center justify-center">
                  <Bell className={`w-3.5 h-3.5 ${notifications > 0 ? 'text-white' : 'text-white/50'}`} />
                  {notifications > 0 && (
                    <div className="absolute -top-1 -right-1 bg-red-500 text-white text-[8px] font-bold px-1 rounded-full border border-[#0a0a0a] min-w-[14px] text-center shadow-[0_0_5px_rgba(239,68,68,0.5)]">
                       {notifications > 99 ? '99+' : notifications}
                    </div>
                  )}
               </div>
            </motion.button>
          )}
       </motion.div>
    </div>
  );
}
