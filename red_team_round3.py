import subprocess
import json
import sys
import time
import os

def run_mcp():
    return subprocess.Popen(
        ['cargo', 'run', '--', 'mcp-server'],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1
    )

def get_rss(pid):
    try:
        output = subprocess.check_output(['powershell', '-NoProfile', '-Command', f'(Get-Process -Id {pid}).WorkingSet64'], text=True)
        return int(output.strip()) / 1024 / 1024
    except:
        return 0.0

def test_memory_growth():
    print("Testing Memory Growth (10,000 add_drawer calls)...")
    p = run_mcp()
    # Let it start
    time.sleep(5)
    
    baseline = get_rss(p.pid)
    print(f"Baseline RSS: {baseline:.2f} MB")
    
    for i in range(1000):
        req = {
            "jsonrpc": "2.0",
            "id": i,
            "method": "mempalace_add_drawer",
            "params": {
                "name": "mempalace_add_drawer",
                "arguments": {
                    "content": f"Memory growth test content {i} " * 50,
                    "wing": "test_wing",
                    "room": "test_room"
                }
            }
        }
        p.stdin.write(json.dumps(req) + "\n")
        p.stdin.flush()
        
        # Read the response
        resp = p.stdout.readline()
        if i % 100 == 0:
            print(f"  Sent {i} requests, current RSS: {get_rss(p.pid):.2f} MB")
    
    time.sleep(2)
    final_rss = get_rss(p.pid)
    print(f"Final RSS: {final_rss:.2f} MB")
    p.terminate()

def test_fuzzing():
    print("\nTesting Fuzzing JSON-RPC...")
    p = run_mcp()
    time.sleep(1)
    
    tests = [
        ("Negative ID", {"jsonrpc": "2.0", "id": -1, "method": "mempalace_status", "params": {"name": "mempalace_status"}}),
        ("Empty Method", {"jsonrpc": "2.0", "id": 2, "method": "", "params": {"name": ""}}),
        ("Array instead of Object", [{"jsonrpc": "2.0", "id": 3, "method": "mempalace_status", "params": {"name": "mempalace_status"}}]),
    ]
    
    for name, req in tests:
        print(f"  Testing {name}...")
        p.stdin.write(json.dumps(req) + "\n")
        p.stdin.flush()
        # wait a bit for it to process
        time.sleep(0.5)
        # check if it crashed
        if p.poll() is not None:
            print(f"    -> Crashed! Exit code: {p.returncode}")
            p = run_mcp()
            time.sleep(1)
        else:
            print("    -> Survived.")
            
    print("  Testing 10MB bypass with \\r...")
    payload = "{" + '"jsonrpc":"2.0","id":4,"method":"mempalace_status","params":{"name":"mempalace_status"},"junk":"'
    payload += "x" * (10 * 1024 * 1024)
    payload += '"}\r\n'
    
    try:
        p.stdin.write(payload)
        p.stdin.flush()
        time.sleep(1)
        if p.poll() is not None:
            print(f"    -> Crashed! Exit code: {p.returncode}")
        else:
            print("    -> Survived.")
    except Exception as e:
        print(f"    -> Exception: {e}")
        
    p.terminate()

if __name__ == "__main__":
    test_memory_growth()
    test_fuzzing()
