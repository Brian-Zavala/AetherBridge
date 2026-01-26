
import os
import sys
import json
import urllib.request
import time

# Configuration
API_BASE = "http://localhost:8080/v1"
API_KEY = "dummy-key"  # AetherBridge doesn't check this yet

def test_chat_completion():
    print(f"Testing Chat Completion against {API_BASE}...")

    url = f"{API_BASE}/chat/completions"
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {API_KEY}"
    }
    data = {
        "model": "google-bridge",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Hello! Can you hear me?"}
        ],
        "stream": False
    }

    try:
        req = urllib.request.Request(
            url,
            data=json.dumps(data).encode('utf-8'),
            headers=headers,
            method="POST"
        )

        start_time = time.time()
        with urllib.request.urlopen(req) as response:
            body = response.read().decode('utf-8')
            status = response.status

        duration = time.time() - start_time

        print(f"Status: {status}")
        print(f"Duration: {duration:.2f}s")

        try:
            json_response = json.loads(body)
            print("Response JSON:")
            print(json.dumps(json_response, indent=2))

            # Basic validation
            if "choices" in json_response and len(json_response["choices"]) > 0:
                print("\n[SUCCESS] Received valid OpenAI-compatible response structure.")
            else:
                print("\n[FAILURE] Response missing 'choices' array.")

        except json.JSONDecodeError:
            print(f"Failed to parse JSON response: {body}")

    except urllib.error.URLError as e:
        print(f"\n[FAILURE] Request failed: {e}")
        if hasattr(e, 'read'):
            print(e.read().decode('utf-8'))

if __name__ == "__main__":
    test_chat_completion()
