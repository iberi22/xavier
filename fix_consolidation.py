import re
import os

filepath = 'src/consolidation/mod.rs'
with open(filepath, 'r') as f:
    content = f.read()

# Fix E0277: managed.doc.id is Option<String>, clone() returns Option<String>,
# but "let Some(doc_id) = managed.doc.id.clone()" is used, which might be the issue if it's not handled correctly.
# Wait, if doc.id is Option<String>, clone() is Option<String>.
# The error E0277 says "the size for values of type 'str' cannot be known at compilation time"
# at "let Some(doc_id) = managed.doc.id.clone()".
# This happens if it tries to bind to str instead of String.

content = content.replace("let Some(doc_id) = managed.doc.id.clone()", "let Some(doc_id) = managed.doc.id.as_ref()")

with open(filepath, 'w') as f:
    f.write(content)
