import sqlite3, os

print("=" * 60)
print("XAVIER MEMORY DB CHECK")
print("=" * 60)

# 1. Check xavier_memory.db
memory_db = "E:/scripts-python/xavier/xavier_memory.db"
print(f"\n--- xavier_memory.db ({os.path.getsize(memory_db)} bytes) ---")
conn = sqlite3.connect(memory_db)
c = conn.cursor()
c.execute("SELECT name FROM sqlite_master WHERE type='table'")
tables = c.fetchall()
for t in tables:
    name = t[0]
    c.execute(f'SELECT COUNT(*) FROM "{name}"')
    count = c.fetchone()[0]
    print(f"  {name}: {count} rows")
    if count > 0 and name in ("files", "documents", "chunks", "nodes", "memory_entries", "embeddings", "xavier_memory"):
        c.execute(f'SELECT * FROM "{name}" LIMIT 3')
        cols = [d[1] for d in c.description]
        print(f"    cols: {cols}")
        for row in c.fetchall():
            row_str = str(row)
            if len(row_str) > 200:
                row_str = row_str[:200] + "..."
            print(f"    {row_str}")
conn.close()

# 2. Check code_graph.db
cg_db = "E:/scripts-python/xavier/data/code_graph.db"
print(f"\n--- code_graph.db ({os.path.getsize(cg_db)} bytes) ---")
conn = sqlite3.connect(cg_db)
c = conn.cursor()
c.execute("SELECT name FROM sqlite_master WHERE type='table'")
tables = c.fetchall()
for t in tables:
    name = t[0]
    c.execute(f'SELECT COUNT(*) FROM "{name}"')
    count = c.fetchone()[0]
    print(f"  {name}: {count} rows")
    if count > 0:
        c.execute(f'SELECT * FROM "{name}" LIMIT 2')
        cols = [d[1] for d in c.description]
        print(f"    cols: {cols}")
        for row in c.fetchall():
            row_str = str(row)
            if len(row_str) > 300:
                row_str = row_str[:300] + "..."
            # Check if any row mentions xavier source files
            if any(kw in str(row).lower() for kw in ("src/", "main.rs", "lib.rs", "xavier", "health", "license", "scoring")):
                print(f"    ⭐ {row_str}")
            else:
                print(f"    {row_str}")
conn.close()
