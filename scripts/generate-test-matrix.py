import json
import glob
import sys
import os

def generate_matrix():
    suites = sorted([f.split('/')[-1].replace('.rs', '') for f in glob.glob('tests/suite_[0-9][0-9].rs')])
    if not suites:
        print("No suites found", file=sys.stderr)
        sys.exit(1)
    matrix = {"suite": suites}
    print(json.dumps(matrix))

if __name__ == '__main__':
    generate_matrix()
