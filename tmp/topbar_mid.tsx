      }

      try {
        const available = await probeMarketplaceAvailable();
        setModules((prev) =>
          prev.marketplace === available ? prev : { ...prev, marketplace: available },
        );
      } finally {
        setInternalLoading(false);
      }
    };

    fetchMetrics();
    const metricsInterval = setInterval(fetchMetrics, 3000);
    const timeInterval = setInterval(() => setTime(new Date()), 1000);

    let unlisten: (() => void) | undefined;
    let notifyInterval: ReturnType<typeof setInterval> | undefined;

    if (isTauri) {
      import("@tauri-apps/api/event")
        .then(({ listen }) => {
          listen<any>("new-notification", () => {
            fetchMetrics();
          }).then((fn) => { unlisten = fn; });
        })
        .catch((err) => console.debug("Error listening for events:", err));
    } else {
      notifyInterval = setInterval(fetchMetrics, 30000);
    }

    return () => {
      clearInterval(metricsInterval);
      clearInterval(timeInterval);
      if (notifyInterval) clearInterval(notifyInterval);
      if (unlisten) unlisten();
    };
  }, []);

  const spring = { type: "spring" as const, stiffness: 200, damping: 25 };

  const openMessaging = (platform: MessagingPlatform) => {
    setMessagingTab(platform);
    setShowMessaging(true);
  };

  const handleTriggerSync = async () => {
    if (isSyncing) return;
    setIsSyncing(true);
    const startTime = performance.now();
    try {
      const token = getToken();
      const client = new ApiClient(token);
      await client.syncPush();
      await client.syncPull();

      const elapsed = Math.round(performance.now() - startTime);
      setSyncLatencyMs(elapsed);
      setLastSyncTime(new Date());

      if (typeof window !== "undefined" && window.dispatchEvent) {
        window.dispatchEvent(
          new CustomEvent("xavier-toast", {
            detail: {
              message: `Memory sync completed successfully (${elapsed}ms)`,
              type: "success",
            },
          }),
        );
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : "Sync operation failed";
      if (typeof window !== "undefined" && window.dispatchEvent) {
        window.dispatchEvent(
          new CustomEvent("xavier-error-toast", {
            detail: { message: `Sync failed: ${msg}`, type: "error" },
          }),
        );
      }
    } finally {
      setIsSyncing(false);
    }
  };

  const handleSaveRemoteUrl = (url: string | null) => {
    setRemoteUrl(url);
    const active = getRemoteUrl();
    setRemoteUrlState(active);
    setShowNodeModal(false);
  };

  return (
    <>
      <div className="absolute inset-0 z-[60] pointer-events-none overflow-hidden">
        {/* Left Group */}
        <motion.div
          layout
          transition={spring}
          className={`flex gap-2 pointer-events-auto ${isModalOpen ? "absolute left-2 lg:left-4 top-1/2 -translate-y-1/2 flex-col items-start z-[60]" : "absolute left-4 md:left-6 top-6 flex-row items-start"}`}
        >
