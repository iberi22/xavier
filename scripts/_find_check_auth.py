import glob

for f in sorted(glob.glob('E:/scripts-python/xavier/src/**/*.rs', recursive=True)):
    with open(f, encoding='utf-8', errors='ignore') as fh:
        content = fh.read()
        if 'pub fn check_auth' in content:
            print('FILE:', f)
            lines = content.split('\n')
            for j, line in enumerate(lines):
                if 'fn check_auth' in line:
                    for k in range(j, min(j+15, len(lines))):
                        print(f'  {k+1}: {lines[k][:200]}')
                    break
            break
else:
    print('No pub fn check_auth found')
