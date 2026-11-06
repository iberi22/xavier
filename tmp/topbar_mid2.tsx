          {/* Time & Date Pill */}
          {modules.time && (
            <motion.div
              layout
              transition={spring}
              className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-3 py-1 flex items-center gap-2 h-7 text-white/80 shrink-0"
            >
              <span className="font-mono text-[10px] hidden md:inline-block">
                {time.toLocaleDateString(undefined, { month: "numeric", day: "numeric" })}
              </span>
              <div className="w-px h-2.5 bg-white/20 hidden md:block" />
              <span className="font-mono text-[10px] min-w-[50px] text-center">
                {time.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}
              </span>
            </motion.div>
          )}

          {/* SWAL Genesis Founder Node Telemetry Badge */}
          {modules.founder && (
            <div className="relative">
              <motion.button
                layout
                transition={spring}
                type="button"
                onClick={() => setShowFounderCard((prev) => !prev)}
                className="bg-[#0a0a0a]/80 backdrop-blur-md border border-amber-500/30 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-1.5 h-7 shrink-0 hover:bg-amber-500/10 transition-colors"
                title="SWAL Genesis Founder Node Telemetry"
                aria-label="Founder Node Telemetry HUD Card"
              >
                <Crown className="w-3 h-3 text-amber-400" />
                <span className="font-mono text-[9px] text-amber-300 font-bold tracking-wider uppercase hidden sm:inline-block">
                  Founder Node
                </span>
                <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
              </motion.button>

              <AnimatePresence>
                {showFounderCard && (
                  <motion.div
                    initial={{ opacity: 0, y: 10, scale: 0.95 }}
                    animate={{ opacity: 1, y: 0, scale: 1 }}
                    exit={{ opacity: 0, y: 10, scale: 0.95 }}
                    className="absolute left-0 top-full mt-2 z-[80]"
                  >
                    <FounderNodeStatusCard onClose={() => setShowFounderCard(false)} />
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          )}

          <OperationModeBadge />
          <WorkspaceSelector />

          {/* System Resources Pill */}
          {modules.resources && (
            <motion.div
              layout
              transition={spring}
              className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-3 py-1 flex items-center gap-3 h-7 shrink-0 hidden lg:flex"
            >
              {isLoading ? (
                <div className="flex items-center gap-1.5 px-1">
                  <LoadingSpinner size={12} />
                  <span className="font-mono text-[10px] text-white/50">Loading...</span>
                </div>
              ) : (
                <>
                  <div
                    className="flex items-center gap-1 text-[10px] text-white/70"
                    title={`Memory: ${metrics.ram_used_gb.toFixed(1)}GB / ${metrics.ram_total_gb.toFixed(1)}GB`}
                  >
                    <Database className="w-3 h-3 text-blue-400" />
                    <span className="font-mono">{Math.round(metrics.ram_used_gb)}G</span>
                  </div>
                  <div
                    className="flex items-center gap-1 text-[10px] text-white/70"
                    title={`CPU: ${Math.round(metrics.cpu_percent)}%`}
                  >
                    <Activity className="w-3 h-3 text-red-400" />
                    <span className="font-mono">{Math.round(metrics.cpu_percent)}%</span>
                  </div>
                  <div className="flex items-center gap-1 text-[10px] text-[#39ff14]" title="GPU: ON">
                    <Zap className="w-3 h-3 fill-[#39ff14]/20" />
                  </div>
                </>
              )}
            </motion.div>
          )}

          {/* Communication Channels */}
          {modules.channels && (
            <motion.div
              layout
              transition={spring}
              className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-2 h-7 shrink-0 hidden md:flex"
            >
              <button
                type="button"
                onClick={() => openMessaging("discord")}
