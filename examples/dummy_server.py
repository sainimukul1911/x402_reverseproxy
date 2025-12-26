#!/usr/bin/env python3
"""
Simple Python API server for testing the x402 reverse proxy.

This server provides a few endpoints that can be used to test
the proxy's rate limiting and payment functionality.

Usage:
    python dummy_server.py [port]
    
Default port: 3000
"""

import json
import sys
from http.server import HTTPServer, BaseHTTPRequestHandler
from datetime import datetime

class DummyAPIHandler(BaseHTTPRequestHandler):
    """Simple API handler for testing."""
    
    request_count = 0
    
    def log_message(self, format, *args):
        """Custom log format with timestamp."""
        print(f"[{datetime.now().isoformat()}] {args[0]}")
    
    def send_json_response(self, status_code: int, data: dict):
        """Send a JSON response."""
        self.send_response(status_code)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps(data, indent=2).encode())
    
    def do_GET(self):
        """Handle GET requests."""
        DummyAPIHandler.request_count += 1
        
        if self.path == '/':
            self.send_json_response(200, {
                'message': 'Hello from the upstream API!',
                'server': 'dummy-python-server',
                'request_number': DummyAPIHandler.request_count,
                'timestamp': datetime.now().isoformat()
            })
        
        elif self.path == '/api/data':
            self.send_json_response(200, {
                'data': [
                    {'id': 1, 'name': 'Item 1'},
                    {'id': 2, 'name': 'Item 2'},
                    {'id': 3, 'name': 'Item 3'},
                ],
                'total': 3,
                'request_number': DummyAPIHandler.request_count
            })
        
        elif self.path == '/api/expensive':
            # Simulate an expensive operation
            self.send_json_response(200, {
                'result': 'This is an expensive API call',
                'cost': '0.001 USDC',
                'request_number': DummyAPIHandler.request_count
            })
        
        elif self.path == '/api/status':
            self.send_json_response(200, {
                'status': 'healthy',
                'uptime': 'running',
                'total_requests': DummyAPIHandler.request_count
            })
        
        elif self.path.startswith('/api/user/'):
            user_id = self.path.split('/')[-1]
            self.send_json_response(200, {
                'user_id': user_id,
                'name': f'User {user_id}',
                'email': f'user{user_id}@example.com'
            })
        
        else:
            self.send_json_response(404, {
                'error': 'Not found',
                'path': self.path
            })
    
    def do_POST(self):
        """Handle POST requests."""
        DummyAPIHandler.request_count += 1
        
        content_length = int(self.headers.get('Content-Length', 0))
        body = self.rfile.read(content_length).decode() if content_length > 0 else ''
        
        try:
            data = json.loads(body) if body else {}
        except json.JSONDecodeError:
            data = {'raw': body}
        
        self.send_json_response(201, {
            'message': 'Resource created',
            'received': data,
            'request_number': DummyAPIHandler.request_count
        })


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 3000
    
    server = HTTPServer(('0.0.0.0', port), DummyAPIHandler)
    
    print(f"""
╔════════════════════════════════════════════════════════════╗
║           Dummy Python API Server for x402 Proxy           ║
╠════════════════════════════════════════════════════════════╣
║  Server running on http://localhost:{port:<24}║
║                                                            ║
║  Available endpoints:                                      ║
║    GET  /              - Hello message                     ║
║    GET  /api/data      - Sample data list                  ║
║    GET  /api/expensive - Simulated expensive call          ║
║    GET  /api/status    - Server status                     ║
║    GET  /api/user/:id  - Get user by ID                    ║
║    POST /api/*         - Echo POST data                    ║
║                                                            ║
║  Press Ctrl+C to stop                                      ║
╚════════════════════════════════════════════════════════════╝
""")
    
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nShutting down server...")
        server.shutdown()


if __name__ == '__main__':
    main()
