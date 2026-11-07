with open("panel-ui/src/components/GraphView.tsx", "r") as f:
    content = f.read()

import_hook = 'import { useTheme } from "../lib/theme/theme-provider";\n'
if import_hook not in content:
    content = content.replace('import type { GraphData, GraphLink, GraphNode } from "../types";',
                              'import type { GraphData, GraphLink, GraphNode } from "../types";\nimport { useTheme } from "../lib/theme/theme-provider";')

hook_init = 'const { resolvedTheme } = useTheme();\n  const isLight = resolvedTheme === "studio-bone" || resolvedTheme === "light";\n'
if "const { resolvedTheme } = useTheme();" not in content:
    content = content.replace('const fgRef = useRef<any>(null);', hook_init + '  const fgRef = useRef<any>(null);')

# Update paintNode dependencies
content = content.replace(', [hoveredNode, expandedNodes, linkCounts, maxLinks]', ', [hoveredNode, expandedNodes, linkCounts, maxLinks, isLight]')

# Update paintNode logic
content = content.replace('ctx.fillStyle = isHovered\n        ? "#fff"\n        :', 'ctx.fillStyle = isHovered\n        ? (isLight ? "#1c1c1f" : "#fff")\n        :')
content = content.replace('ctx.fillStyle = isExp ? "#0a0a0a" : "rgba(255, 255, 255, 0.8)";', 'ctx.fillStyle = isExp ? (isLight ? "#ffffff" : "#0a0a0a") : (isLight ? "rgba(28, 28, 31, 0.8)" : "rgba(255, 255, 255, 0.8)");')

# Update paintLink dependencies
content = content.replace(', [hoveredNode]', ', [hoveredNode, isLight]')

# Update paintLink logic
content = content.replace('? "rgba(57, 255, 20, 0.8)"\n        : "rgba(57, 255, 20, 0.15)";', '? "rgba(57, 255, 20, 0.8)"\n        : (isLight ? "rgba(28, 28, 31, 0.15)" : "rgba(57, 255, 20, 0.15)");')


with open("panel-ui/src/components/GraphView.tsx", "w") as f:
    f.write(content)
