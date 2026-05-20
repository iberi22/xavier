import os
import re
import json

def get_technical_debt(root_dir):
    patterns = {
        "TODO": re.compile(r"\bTODO\b"),
        "FIXME": re.compile(r"\bFIXME\b"),
        "BUG": re.compile(r"\bBUG\b"),
        "DEBT": re.compile(r"\bDEBT\b")
    }
    counts = {k: 0 for k in patterns}
    
    for root, dirs, files in os.walk(root_dir):
        if "target" in dirs: dirs.remove("target")
        if ".git" in dirs: dirs.remove(".git")
        
        for file in files:
            if file.endswith((".rs", ".py", ".js", ".md")):
                path = os.path.join(root, file)
                try:
                    with open(path, "r", encoding="utf-8") as f:
                        content = f.read()
                        for key, pattern in patterns.items():
                            counts[key] += len(pattern.findall(content))
                except:
                    pass
    return counts

def get_complexity_metrics(root_dir):
    # Analyze Rust files for function length and nesting
    metrics = []
    for root, dirs, files in os.walk(root_dir):
        if "target" in dirs: dirs.remove("target")
        for file in files:
            if file.endswith(".rs"):
                path = os.path.join(root, file)
                try:
                    with open(path, "r", encoding="utf-8") as f:
                        lines = f.readlines()
                        file_size = len(lines)
                        # Simple heuristic: count functions and their lengths
                        fn_count = 0
                        max_nesting = 0
                        current_nesting = 0
                        for line in lines:
                            if "fn " in line: fn_count += 1
                            current_nesting += line.count("{") - line.count("}")
                            max_nesting = max(max_nesting, current_nesting)
                        
                        metrics.append({
                            "path": os.path.relpath(path, root_dir),
                            "lines": file_size,
                            "functions": fn_count,
                            "max_nesting": max_nesting
                        })
                except:
                    pass
    
    # Sort by lines descending
    metrics.sort(key=lambda x: x["lines"], reverse=True)
    return metrics[:10]

if __name__ == "__main__":
    root = "."
    debt = get_technical_debt(root)
    complexity = get_complexity_metrics(root)
    
    output = {
        "technical_debt": debt,
        "top_files_by_size": complexity
    }
    
    print(json.dumps(output, indent=2))
