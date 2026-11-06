import re

def process_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()

    # Replacing missing text colors
    content = re.sub(r'text-white/80', r'text-[var(--foreground)]/80 dark:text-white/80', content)
    content = re.sub(r'text-white/70', r'text-[var(--foreground)]/70 dark:text-white/70', content)
    content = re.sub(r'text-white/60', r'text-[var(--foreground)]/60 dark:text-white/60', content)
    content = re.sub(r'text-white/65', r'text-[var(--foreground)]/65 dark:text-white/65', content)
    content = re.sub(r'text-white/50', r'text-[var(--foreground)]/50 dark:text-white/50', content)
    content = re.sub(r'text-white/40', r'text-[var(--foreground)]/40 dark:text-white/40', content)
    content = re.sub(r'text-white/30', r'text-[var(--foreground)]/30 dark:text-white/30', content)
    content = re.sub(r'text-white/25', r'text-[var(--foreground)]/25 dark:text-white/25', content)
    content = re.sub(r'text-white/20', r'text-[var(--foreground)]/20 dark:text-white/20', content)

    # Replacing bg colors
    content = re.sub(r'bg-white/5', r'bg-[var(--foreground)]/5 dark:bg-white/5', content)
    content = re.sub(r'bg-white/10', r'bg-[var(--foreground)]/10 dark:bg-white/10', content)
    content = re.sub(r'bg-white/\[0\.02\]', r'bg-[var(--foreground)]/[0.02] dark:bg-white/[0.02]', content)

    content = re.sub(r'bg-black/10', r'bg-[var(--surface-card)] dark:bg-black/10', content)
    content = re.sub(r'bg-black/20', r'bg-[var(--surface-card)] dark:bg-black/20', content)
    content = re.sub(r'bg-black/30', r'bg-[var(--surface-card)] dark:bg-black/30', content)

    # Replacing border colors
    content = re.sub(r'border-white/10', r'border-[var(--border-subtle)] dark:border-white/10', content)
    content = re.sub(r'border-white/20', r'border-[var(--border-subtle)] dark:border-white/20', content)
    content = re.sub(r'border-white/\[0\.03\]', r'border-[var(--border-subtle)] dark:border-white/[0.03]', content)
    content = re.sub(r'border-white/\[0\.04\]', r'border-[var(--border-subtle)] dark:border-white/[0.04]', content)
    content = re.sub(r'border-white/\[0\.05\]', r'border-[var(--border-subtle)] dark:border-white/[0.05]', content)
    content = re.sub(r'border-white/\[0\.06\]', r'border-[var(--border-subtle)] dark:border-white/[0.06]', content)

    with open(filepath, 'w') as f:
        f.write(content)

process_file('panel-ui/src/pages/Settings/Security.tsx')
