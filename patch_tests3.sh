#!/bin/bash
cat << 'INNER_EOF' > patch.py
import re

with open("src/server/v1_api.rs", "r") as f:
    content = f.read()

# Replace test/decision/1 with test_decision_1
content = content.replace('"user_id": "test/decision/1"', '"user_id": "test_decision_1"')

# Replace test/fact/1 with test_fact_1
content = content.replace('"user_id": "test/fact/1"', '"user_id": "test_fact_1"')

# Replace other/fact/1 with other_fact_1
content = content.replace('"user_id": "other/fact/1"', '"user_id": "other_fact_1"')

# Replace "test/" with "test" for the prefix
content = content.replace('"path_prefix": "test/"', '"path_prefix": "test_"')

with open("src/server/v1_api.rs", "w") as f:
    f.write(content)

INNER_EOF
python3 patch.py
