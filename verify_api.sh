#!/bin/bash
curl -v -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "google-bridge",
    "messages": [
      {"role": "user", "content": "Hello AetherBridge!"}
    ]
  }'
