with open("panel-ui/src/components/GraphCanvas.tsx", "r") as f:
    content = f.read()

import_hook = 'import { useTheme } from "../lib/theme/theme-provider";\n'
if import_hook not in content:
    content = content.replace('import type { GraphData, GraphLink, GraphNode } from "../types";',
                              'import type { GraphData, GraphLink, GraphNode } from "../types";\nimport { useTheme } from "../lib/theme/theme-provider";')

hook_init = 'const { resolvedTheme } = useTheme();\n  const isLight = resolvedTheme === "studio-bone" || resolvedTheme === "light";\n'
if "const { resolvedTheme } = useTheme();" not in content:
    content = content.replace('const fgRef = useRef<any>(null);', hook_init + '  const fgRef = useRef<any>(null);')

# Update paintNode dependencies
content = content.replace(', [hoveredNode, selectedNode, linkCounts, maxLinks, isCodeMode]', ', [hoveredNode, selectedNode, linkCounts, maxLinks, isCodeMode, isLight]')

# Update paintNode logic
content = content.replace('ctx.fillStyle = isHovered ? "#ffffff" : nodeColor;', 'ctx.fillStyle = isHovered ? (isLight ? "#1c1c1f" : "#ffffff") : nodeColor;')
content = content.replace('ctx.shadowColor = isHovered || isSelected ? "#ffffff" : nodeColor;', 'ctx.shadowColor = isHovered || isSelected ? (isLight ? "#1c1c1f" : "#ffffff") : nodeColor;')
content = content.replace('ctx.fillStyle = isSelected ? "#39ff14" : "rgba(255, 255, 255, 0.85)";', 'ctx.fillStyle = isSelected ? "#39ff14" : (isLight ? "rgba(28, 28, 31, 0.85)" : "rgba(255, 255, 255, 0.85)");')

# Update paintLink dependencies
content = content.replace(', [hoveredNode, selectedNode]', ', [hoveredNode, selectedNode, isLight]')

# Update paintLink logic
content = content.replace('? "rgba(255, 255, 255, 0.5)"\n          : "rgba(255, 255, 255, 0.12)";', '? (isLight ? "rgba(28, 28, 31, 0.5)" : "rgba(255, 255, 255, 0.5)")\n          : (isLight ? "rgba(28, 28, 31, 0.12)" : "rgba(255, 255, 255, 0.12)");')

with open("panel-ui/src/components/GraphCanvas.tsx", "w") as f:
    f.write(content)
