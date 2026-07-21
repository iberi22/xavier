import os
import re

def main():
    pub_fn_re = re.compile(r'^\s*(?:pub(?:\([^)]+\))?\s+)?(?:async\s+)?fn\s+([a-zA-Z0-9_]+)\s*\(')
    doc_re = re.compile(r'^\s*///')

    missing = 0
    total = 0
    for root, dirs, files in os.walk('src'):
        for file in files:
            if file.endswith('.rs'):
                path = os.path.join(root, file)
                with open(path, 'r', encoding='utf-8') as f:
                    lines = f.readlines()

                for i, line in enumerate(lines):
                    m = pub_fn_re.match(line)
                    if m:
                        total += 1
                        has_doc = False
                        if i > 0 and doc_re.match(lines[i-1]):
                            has_doc = True
                        if not has_doc:
                            missing += 1

    print(f"Total public functions: {total}")
    print(f"Missing docstrings: {missing}")

if __name__ == '__main__':
    main()
