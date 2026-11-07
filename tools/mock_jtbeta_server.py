#!/usr/bin/env python3
"""Stand-in for api.github.com and www.patreon.com to test the plugin offline.

  python3 tools/mock_jtbeta_server.py --port 8767 --zip /path/to/jtbeta.zip \
      --github-token GOODTOKEN --patreon-cookie GOODCOOKIE [--gated]

GitHub:  GET /repos/JTFPGA/jtbeta/contents/jtbeta.zip   (Authorization: token <t>)  -> zip or 401
Patreon: GET /jotego                                     -> page with the campaign id
         GET /api/posts?...                              (Cookie: session_id=<c>)    -> page 1 (no match, next) / page 2 (match)
         GET /cdn/jtbeta.zip                             -> zip
"""
import argparse
import json
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import urlparse, parse_qs


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--port', type=int, default=8767)
    parser.add_argument('--zip', required=True)
    parser.add_argument('--github-token', default='GOODTOKEN')
    parser.add_argument('--patreon-cookie', default='GOODCOOKIE')
    parser.add_argument('--gated', action='store_true', help='the jtbeta post exists but is not viewable')
    args = parser.parse_args()
    zip_bytes = open(args.zip, 'rb').read()
    base = f'http://127.0.0.1:{args.port}/'

    class Handler(BaseHTTPRequestHandler):
        def reply(self, status, body, ctype='application/json'):
            self.send_response(status)
            self.send_header('Content-Type', ctype)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            url = urlparse(self.path)
            if url.path == '/repos/JTFPGA/jtbeta/contents/jtbeta.zip':
                if self.headers.get('Authorization') == f'token {args.github_token}':
                    return self.reply(200, zip_bytes, 'application/octet-stream')
                return self.reply(401, json.dumps({'message': 'Bad credentials'}).encode())
            if url.path == '/jotego':
                return self.reply(200, b'<html><script>{"data":{"id":"4242","type":"campaign"}}</script></html>', 'text/html')
            if url.path == '/api/posts':
                if self.headers.get('Cookie') != f'session_id={args.patreon_cookie}':
                    return self.reply(401, json.dumps({'errors': [{'detail': 'unauthorized'}]}).encode())
                q = parse_qs(url.query)
                if q.get('filter[campaign_id]') != ['4242']:
                    return self.reply(400, b'{"errors":[{"detail":"bad campaign"}]}')
                if q.get('page') != ['2']:
                    page = {'data': [{'attributes': {'url': 'https://www.patreon.com/posts/news', 'current_user_can_view': True},
                                      'relationships': {'attachments_media': {'data': [{'id': 'm0'}]}}}],
                            'included': [{'type': 'media', 'id': 'm0', 'attributes': {'file_name': 'news.txt', 'download_url': base + 'cdn/news.txt'}}],
                            'links': {'next': base + 'api/posts?filter[campaign_id]=4242&page=2'}}
                else:
                    page = {'data': [{'attributes': {'url': 'https://www.patreon.com/posts/jtbeta-123', 'current_user_can_view': not args.gated},
                                      'relationships': {'attachments_media': {'data': [{'id': 'm1'}]}}}],
                            'included': [{'type': 'media', 'id': 'm1', 'attributes': {'file_name': 'jtbeta.zip', 'download_url': base + 'cdn/jtbeta.zip'}}],
                            'links': {}}
                return self.reply(200, json.dumps(page).encode())
            if url.path == '/cdn/jtbeta.zip':
                return self.reply(200, zip_bytes, 'application/zip')
            return self.reply(404, b'{"error":"not found"}')

        def log_message(self, fmt, *a):
            print('[mock] ' + fmt % a, flush=True)

    print(f'[mock] serving on {base}', flush=True)
    HTTPServer(('127.0.0.1', args.port), Handler).serve_forever()


if __name__ == '__main__':
    main()
