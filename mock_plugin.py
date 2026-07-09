#!/usr/bin/env python3
import sys
import json

def main():
    try:
        # Read request from stdin
        input_data = sys.stdin.read()
        if not input_data:
            return

        request = json.loads(input_data)

        symbols = []
        for file_info in request.get('files', []):
            # Mock symbol extraction: just return a dummy symbol for each file
            symbols.append({
                "name": "MockPluginSymbol",
                "kind": "Function",
                "lang": request.get('language', 'Unknown'),
                "file_path": file_info.get('path', 'unknown'),
                "start_line": 1,
                "end_line": 1,
                "start_col": 0,
                "end_col": 0
            })

        # Write response to stdout
        response = {
            "symbols": symbols,
            "error": None
        }
        print(json.dumps(response))

    except Exception as e:
        response = {
            "symbols": [],
            "error": str(e)
        }
        print(json.dumps(response))

if __name__ == "__main__":
    main()
