#!/usr/bin/env python3
import sys
import json
import ast

def parse_with_ast(source, file_path):
    """
    Parses Python source code using Python's built-in AST library.
    Extracts classes, methods, functions, and imports with correct positions, signatures,
    and cyclomatic complexity.
    """
    try:
        tree = ast.parse(source, filename=file_path)
    except SyntaxError as e:
        # Gracefully handle syntax errors by returning them in the response error field
        raise ValueError(f"Syntax error at line {e.lineno}, col {e.offset}: {e.msg}")

    class PythonSymbolExtractor(ast.NodeVisitor):
        def __init__(self, file_path):
            self.file_path = file_path
            self.symbols = []
            self.current_parent = None

        def visit_ClassDef(self, node):
            start_line = node.lineno
            end_line = getattr(node, 'end_lineno', start_line)
            start_col = node.col_offset
            end_col = getattr(node, 'end_col_offset', start_col)

            signature = f"class {node.name}"
            if node.bases:
                bases_list = []
                for base in node.bases:
                    if isinstance(base, ast.Name):
                        bases_list.append(base.id)
                    elif isinstance(base, ast.Attribute) and isinstance(base.value, ast.Name):
                        bases_list.append(f"{base.value.id}.{base.attr}")
                if bases_list:
                    signature += f"({', '.join(bases_list)})"

            self.symbols.append({
                "id": None,
                "stable_id": None,
                "name": node.name,
                "kind": "Class",
                "lang": "Python",
                "file_path": self.file_path,
                "start_line": start_line,
                "end_line": end_line,
                "start_col": start_col,
                "end_col": end_col,
                "signature": signature,
                "parent": self.current_parent,
                "complexity": None
            })

            old_parent = self.current_parent
            self.current_parent = node.name
            self.generic_visit(node)
            self.current_parent = old_parent

        def visit_FunctionDef(self, node):
            self.extract_func(node, "Function" if self.current_parent is None else "Method")

        def visit_AsyncFunctionDef(self, node):
            self.extract_func(node, "Function" if self.current_parent is None else "Method")

        def extract_func(self, node, kind):
            start_line = node.lineno
            end_line = getattr(node, 'end_lineno', start_line)
            start_col = node.col_offset
            end_col = getattr(node, 'end_col_offset', start_col)

            # Simple approximation of cyclomatic complexity
            complexity = self.calculate_complexity(node)

            prefix = "async def" if isinstance(node, ast.AsyncFunctionDef) else "def"
            signature = f"{prefix} {node.name}(...)"

            self.symbols.append({
                "id": None,
                "stable_id": None,
                "name": node.name,
                "kind": kind,
                "lang": "Python",
                "file_path": self.file_path,
                "start_line": start_line,
                "end_line": end_line,
                "start_col": start_col,
                "end_col": end_col,
                "signature": signature,
                "parent": self.current_parent,
                "complexity": float(complexity)
            })

            old_parent = self.current_parent
            self.current_parent = node.name
            self.generic_visit(node)
            self.current_parent = old_parent

        def visit_Import(self, node):
            for alias in node.names:
                start_line = node.lineno
                self.symbols.append({
                    "id": None,
                    "stable_id": None,
                    "name": alias.name,
                    "kind": "Import",
                    "lang": "Python",
                    "file_path": self.file_path,
                    "start_line": start_line,
                    "end_line": getattr(node, 'end_lineno', start_line),
                    "start_col": node.col_offset,
                    "end_col": getattr(node, 'end_col_offset', node.col_offset),
                    "signature": f"import {alias.name}",
                    "parent": self.current_parent,
                    "complexity": None
                })

        def visit_ImportFrom(self, node):
            module = node.module or ""
            for alias in node.names:
                start_line = node.lineno
                self.symbols.append({
                    "id": None,
                    "stable_id": None,
                    "name": alias.name,
                    "kind": "Import",
                    "lang": "Python",
                    "file_path": self.file_path,
                    "start_line": start_line,
                    "end_line": getattr(node, 'end_lineno', start_line),
                    "start_col": node.col_offset,
                    "end_col": getattr(node, 'end_col_offset', node.col_offset),
                    "signature": f"from {module} import {alias.name}",
                    "parent": self.current_parent,
                    "complexity": None
                })

        def calculate_complexity(self, node):
            decision_points = 1
            for child in ast.walk(node):
                if isinstance(child, (ast.If, ast.While, ast.For, ast.AsyncFor, ast.Try, ast.ExceptHandler, ast.With, ast.AsyncWith)):
                    decision_points += 1
                elif isinstance(child, ast.BoolOp):
                    decision_points += len(child.values) - 1
            return decision_points

    extractor = PythonSymbolExtractor(file_path)
    extractor.visit(tree)
    return extractor.symbols


def parse_with_tree_sitter(source, file_path):
    """
    Skeleton tree-sitter parser, to be used when tree-sitter packages are available.
    """
    import tree_sitter
    import tree_sitter_python

    # Load the python tree-sitter language
    language = tree_sitter.Language(tree_sitter_python.language())
    parser = tree_sitter.Parser(language)

    tree = parser.parse(bytes(source, "utf8"))
    symbols = []

    def traverse(node, parent_name=None):
        kind = None
        if node.type == "function_definition":
            kind = "Function" if parent_name is None else "Method"
        elif node.type == "class_definition":
            kind = "Class"
        elif node.type in ("import_statement", "import_from_statement"):
            kind = "Import"

        current_node_name = None
        if kind:
            # Try to find a name child
            name_node = None
            for child in node.children:
                if child.type == "identifier":
                    name_node = child
                    break

            if name_node:
                name = source[name_node.start_byte:name_node.end_byte]
                current_node_name = name
                start_point = node.start_point
                end_point = node.end_point

                symbols.append({
                    "id": None,
                    "stable_id": None,
                    "name": name,
                    "kind": kind,
                    "lang": "Python",
                    "file_path": file_path,
                    "start_line": start_point[0] + 1,
                    "end_line": end_point[0] + 1,
                    "start_col": start_point[1],
                    "end_col": end_point[1],
                    "signature": source[node.start_byte:node.end_byte].split("\n")[0],
                    "parent": parent_name,
                    "complexity": 1.0 if kind in ("Function", "Method") else None
                })

        for child in node.children:
            traverse(child, current_node_name or parent_name)

    traverse(tree.root_node)
    return symbols


def main():
    # Handle direct CLI arguments for health-check / version operations
    if len(sys.argv) > 1:
        arg = sys.argv[1].lower().lstrip("-")
        if arg in ("health", "h"):
            print("Success")
            sys.exit(0)
        elif arg in ("version", "v"):
            print("0.1.0")
            sys.exit(0)

    # Otherwise read from stdin
    try:
        input_data = sys.stdin.read()
    except Exception as e:
        print(json.dumps({"symbols": [], "error": f"Failed to read from stdin: {str(e)}"}))
        sys.exit(1)

    if not input_data.strip():
        # Treat as health/no-op parse request
        print(json.dumps({"symbols": [], "error": None}))
        sys.exit(0)

    try:
        request = json.loads(input_data)
    except Exception as e:
        print(json.dumps({"symbols": [], "error": f"Failed to parse input JSON: {str(e)}"}))
        sys.exit(1)

    # Validate request structure
    if "files" not in request:
        print(json.dumps({"symbols": [], "error": "Invalid request: 'files' field is required"}))
        sys.exit(1)

    all_symbols = []
    error_msg = None

    for file_info in request["files"]:
        path = file_info.get("path", "")
        source = file_info.get("source", "")

        try:
            # Attempt to use tree-sitter if available
            try:
                symbols = parse_with_tree_sitter(source, path)
            except ImportError:
                # Fallback to standard AST parsing
                symbols = parse_with_ast(source, path)

            all_symbols.extend(symbols)
        except Exception as e:
            error_msg = str(e)
            break

    response = {
        "symbols": all_symbols,
        "error": error_msg
    }

    print(json.dumps(response, indent=2))


if __name__ == "__main__":
    main()
