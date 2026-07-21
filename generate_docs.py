import os
import re

def process_file(filepath):
    with open(filepath, 'r', encoding='utf-8') as f:
        lines = f.readlines()

    # Matches `pub fn`, `pub(crate) fn`, `pub async fn`, etc.
    pub_fn_re = re.compile(r'^(\s*)(pub(?:\([^)]+\))?\s+(?:async\s+)?fn\s+([a-zA-Z0-9_]+)\s*\()')
    doc_re = re.compile(r'^\s*///')
    attr_re = re.compile(r'^\s*#\[')

    new_lines = []

    for i, line in enumerate(lines):
        m = pub_fn_re.match(line)
        if m:
            indent = m.group(1)
            fn_name = m.group(3)

            # Check if there's already a docstring
            # Need to look backwards skipping attributes and empty lines
            j = i - 1
            has_doc = False
            while j >= 0:
                prev = lines[j].strip()
                if not prev:
                    j -= 1
                    continue
                if prev.startswith('#['):
                    j -= 1
                    continue
                if prev.startswith('///'):
                    has_doc = True
                    break
                break

            if not has_doc:
                # Add a docstring
                docstring = f"{indent}/// {fn_name.replace('_', ' ').capitalize()}.\n"
                new_lines.append(docstring)

        new_lines.append(line)

    with open(filepath, 'w', encoding='utf-8') as f:
        f.writelines(new_lines)

def main():
    modules = [d for d in os.listdir('src') if os.path.isdir(os.path.join('src', d))]

    os.makedirs('docs/source', exist_ok=True)

    # Analyze cross-references
    # We will do a simple pass to see which modules use other modules
    # by looking for `use crate::module_name`
    module_deps = {mod: set() for mod in modules}

    use_re = re.compile(r'^\s*use\s+crate::([a-zA-Z0-9_]+)')

    for mod in modules:
        mod_path = os.path.join('src', mod)
        for root, dirs, files in os.walk(mod_path):
            for file in files:
                if file.endswith('.rs'):
                    filepath = os.path.join(root, file)
                    with open(filepath, 'r', encoding='utf-8') as f:
                        for line in f:
                            m = use_re.search(line)
                            if m:
                                dep = m.group(1)
                                if dep in modules and dep != mod:
                                    module_deps[mod].add(dep)

    for mod in modules:
        mod_path = os.path.join('src', mod)

        # generate SRC.md for each module in docs/source/
        src_md_path = os.path.join('docs/source', f'{mod}_SRC.md')

        with open(src_md_path, 'w', encoding='utf-8') as f:
            f.write(f"# {mod} Module SRC\n\n")
            f.write(f"## Overview\n\nDocumentation for the `{mod}` module.\n\n")

            f.write("## Dependencies\n\n")
            deps = module_deps[mod]
            if deps:
                f.write("This module depends on the following modules:\n")
                for dep in sorted(deps):
                    f.write(f"- [{dep}]({dep}_SRC.md)\n")
            else:
                f.write("This module has no internal dependencies.\n")
            f.write("\n")

            f.write("## Components\n\n")

            for root, dirs, files in os.walk(mod_path):
                for file in files:
                    if file.endswith('.rs'):
                        rel_path = os.path.relpath(os.path.join(root, file), 'src')
                        f.write(f"- `{rel_path}`\n")

                        # Add docstrings to public functions
                        process_file(os.path.join(root, file))

        print(f"Processed {mod}")

if __name__ == '__main__':
    main()
