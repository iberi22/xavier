import urllib.request
import json
import time
import sys

XAVIER_URL = "http://localhost:8003"
AUTH_TOKEN = "verification_token" # Placeholder, adjust if needed

def check_health():
    print(f"Checking Xavier health at {XAVIER_URL}...")
    try:
        req = urllib.request.Request(f"{XAVIER_URL}/health")
        with urllib.request.urlopen(req, timeout=5) as response:
            status = response.getcode()
            if status == 200:
                print("[OK] Xavier is responsive.")
                return True
            else:
                print(f"[FAIL] Xavier returned status {status}.")
                return False
    except Exception as e:
        print(f"[ERROR] Could not connect to Xavier: {e}")
        return False

def verify_save_retrieve():
    path = "system/verification/diag_test"
    content = f"Deterministic verification token: {time.time()}"
    
    print(f"Verifying save/retrieve integrity for path: {path}...")
    
    # Save
    save_data = json.dumps({
        "path": path,
        "content": content,
        "kind": "verification"
    }).encode('utf-8')
    
    try:
        req = urllib.request.Request(
            f"{XAVIER_URL}/memory/add",
            data=save_data,
            headers={'Content-Type': 'application/json'}
        )
        with urllib.request.urlopen(req, timeout=5) as response:
            if response.getcode() != 200:
                print("[FAIL] Save failed.")
                return False
        
        # Search
        search_data = json.dumps({
            "query": content,
            "path": path,
            "limit": 1
        }).encode('utf-8')
        
        req = urllib.request.Request(
            f"{XAVIER_URL}/memory/search",
            data=search_data,
            headers={'Content-Type': 'application/json'}
        )
        
        with urllib.request.urlopen(req, timeout=5) as response:
            res_json = json.loads(response.read().decode('utf-8'))
            results = res_json.get("results", [])
            if not results:
                print("[FAIL] Retrieval failed: no results.")
                return False
            
            retrieved_content = results[0].get("content", "")
            if retrieved_content == content:
                print("[OK] Save/Retrieve integrity verified.")
                return True
            else:
                print("[FAIL] Content mismatch.")
                return False
                
    except Exception as e:
        print(f"[ERROR] Memory verification failed: {e}")
        return False

if __name__ == "__main__":
    is_healthy = check_health()
    if is_healthy:
        is_integral = verify_save_retrieve()
        sys.exit(0 if is_integral else 1)
    else:
        sys.exit(1)
