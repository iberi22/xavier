import re

with open("panel-ui/src/components/MeshConfig.tsx", "r") as f:
    content = f.read()

# I will carefully replace ONLY inside the return statement of MeshConfig to not break unrelated things,
# even though MeshConfig itself is fully scoped.
# The user wants ONLY panel-ui/src/components/MeshConfig.tsx modified (and e2e).

def replace_class(pattern, repl, text):
    return re.sub(pattern, repl, text)

# text colors
content = replace_class(r'text-white/80', r'text-foreground/80 dark:text-white/80', content)
content = replace_class(r'text-white/40', r'text-foreground/40 dark:text-white/40', content)
content = replace_class(r'text-white/60', r'text-foreground/60 dark:text-white/60', content)
content = replace_class(r'text-white/70', r'text-foreground/70 dark:text-white/70', content)
content = replace_class(r'text-white/30', r'text-foreground/30 dark:text-white/30', content)
content = replace_class(r'text-white/20', r'text-foreground/20 dark:text-white/20', content)
content = replace_class(r'text-white/10', r'text-foreground/10 dark:text-white/10', content)
content = replace_class(r'text-white/5', r'text-foreground/5 dark:text-white/5', content)
content = replace_class(r'text-white/90', r'text-foreground/90 dark:text-white/90', content)

# Instead of blindly replacing `hover:text-white` which conflicts, replace specifically:
content = replace_class(r'hover:text-white ', r'hover:text-foreground hover:dark:text-white ', content)
content = replace_class(r'hover:text-white"', r'hover:text-foreground hover:dark:text-white"', content)
# We also have `text-white` without opacities
content = replace_class(r'text-white ', r'text-foreground dark:text-white ', content)
content = replace_class(r'text-white"', r'text-foreground dark:text-white"', content)

# background colors
content = replace_class(r'bg-white/\[0\.02\]', r'bg-surface-card dark:bg-white/[0.02]', content)
content = replace_class(r'bg-white/\[0\.01\]', r'bg-surface dark:bg-white/[0.01]', content)
content = replace_class(r'bg-white/5', r'bg-foreground/5 dark:bg-white/5', content)
content = replace_class(r'bg-white/10', r'bg-foreground/10 dark:bg-white/10', content)
content = replace_class(r'bg-black/30', r'bg-surface dark:bg-black/30', content)
content = replace_class(r'bg-black/40', r'bg-surface-card dark:bg-black/40', content)

# borders
content = replace_class(r'border-white/\[0\.04\]', r'border-border-subtle dark:border-white/[0.04]', content)
content = replace_class(r'border-white/\[0\.06\]', r'border-border-subtle dark:border-white/[0.06]', content)
content = replace_class(r'border-white/5 ', r'border-border-subtle dark:border-white/5 ', content)
content = replace_class(r'border-white/5"', r'border-border-subtle dark:border-white/5"', content)
content = replace_class(r'border-white/10', r'border-border-subtle dark:border-white/10', content)
content = replace_class(r'hover:border-white/10', r'hover:border-border-subtle hover:dark:border-white/10', content)
content = replace_class(r'hover:border-white/20', r'hover:border-foreground/20 hover:dark:border-white/20', content)

with open("panel-ui/src/components/MeshConfig.tsx", "w") as f:
    f.write(content)
