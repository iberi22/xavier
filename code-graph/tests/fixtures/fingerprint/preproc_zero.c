#if 0 /* disabled */
int dead_a(void) { return 1; }
#else
int live_a(void) { return 2; }
#endif
#if (0) // off
int dead_b(void) { return 3; }
#elif 0L
int dead_c(void) { return 4; }
#else
int live_b(void) { return 5; }
#endif
