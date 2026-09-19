#!/usr/bin/env python3
"""Synthetic Spanish legal text retrieval probe; not OCR, generation or ACL validation."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('/tmp/xavier-legal-rag-http-20260918.json'))
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    token = None
    for line in (root / '.env').read_text().splitlines():
        key, sep, value = line.partition('=')
        if sep and key.strip() == 'XAVIER_TOKEN':
            token = value.strip().strip('"\'')
    if not token:
        raise SystemExit('Canonical environment lacks XAVIER_TOKEN')
    prefix = 'legal-rag-audit/' + str(uuid.uuid4()) + '/'
    documents = [
        ('expediente-a/pago', 'Contrato sintético ALFA. CLÁUSULA PAGO. El precio será de 12500 euros y vence el 30 de noviembre de 2026.'),
        ('expediente-a/terminacion', 'Contrato sintético ALFA. CLÁUSULA TERMINACIÓN. La terminación anticipada exige preaviso escrito de cuarenta y cinco días.'),
        ('expediente-a/confidencialidad', 'Contrato sintético ALFA. CLÁUSULA CONFIDENCIALIDAD. La obligación de reserva permanece durante cinco años después de terminar el contrato.'),
        ('expediente-a/jurisdiccion', 'Contrato sintético ALFA. CLÁUSULA JURISDICCIÓN. Las controversias serán resueltas por los tribunales de Bogotá.'),
        ('expediente-a/garantia', 'Contrato sintético ALFA. CLÁUSULA GARANTÍA. El proveedor reparará defectos durante veinticuatro meses desde la entrega.'),
        ('expediente-b/terminacion', 'Contrato sintético BETA. CLÁUSULA TERMINACIÓN. La terminación anticipada exige preaviso escrito de dos días. Expediente distinto.'),
    ]
    queries = [
        ('pago', '¿Cuál es el precio y vencimiento del pago del contrato ALFA?'),
        ('terminacion', '¿Qué preaviso exige la terminación anticipada del contrato ALFA?'),
        ('confidencialidad', '¿Cuánto dura la confidencialidad después de terminar el contrato ALFA?'),
        ('jurisdiccion', '¿Qué tribunales resuelven controversias del contrato ALFA?'),
        ('garantia', '¿Durante cuánto tiempo se reparan defectos según la garantía del contrato ALFA?'),
    ]
    report = {'scope': __doc__, 'started_at': datetime.now(timezone.utc).isoformat(), 'prefix': prefix, 'queries': [], 'controls': [], 'created': [], 'cleanup': [], 'errors': [],
        'caveats': ['Immediate read-after-write retrieval; no warmup delay.',
                    'An empty filtered result does not prove ACL isolation.',
                    'Scores describe this six-document synthetic probe only.']}
    owned = {}

    def call(route, payload=None):
        data = None if payload is None else json.dumps(payload).encode()
        request = urllib.request.Request('http://127.0.0.1:8006' + route, data=data,
            headers={'X-Xavier-Token': token, 'Content-Type': 'application/json'})
        start = time.monotonic()
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return response.status, json.load(response), round(time.monotonic() - start, 3)
        except urllib.error.HTTPError as error:
            return error.code, {}, round(time.monotonic() - start, 3)

    try:
        for suffix, content in documents:
            path = prefix + suffix
            status, body, seconds = call('/v1/memories', {'content': content, 'path': path,
                'metadata': {'kind': 'document', 'zone': 'atomic', 'title': 'Synthetic legal retrieval audit', 'synthetic': True}})
            identifier = body.get('id')
            if not identifier or status != 200:
                raise RuntimeError('Synthetic insert failed: HTTP ' + str(status))
            owned[identifier] = path
            read_status, read_body, _ = call('/v1/memories/' + urllib.parse.quote(identifier, safe=''))
            stored = read_body.get('memory', {})
            stored_path = stored.get('user_id', stored.get('path')) if isinstance(stored, dict) else None
            if isinstance(stored, dict):
                stored = stored.get('memory', stored.get('content'))
            integrity = read_status == 200 and stored == content
            report['created'].append({'id': identifier, 'path': path, 'seconds': seconds,
                'content_sha256': hashlib.sha256(content.encode()).hexdigest(), 'read_integrity': integrity,
                'path_integrity': stored_path == path})
            if not integrity:
                raise RuntimeError('Inserted content failed readback integrity')
        for expected, query in queries:
            status, body, seconds = call('/v1/memories/search', {'query': query, 'limit': 3, 'mode': 'snippet',
                'filters': {'path_prefix': prefix + 'expediente-a/'}})
            results = body.get('results', [])
            safe_results = []
            outside = 0
            for item in results:
                identifier, path = item.get('id'), item.get('path', '')
                if identifier not in owned or path != owned.get(identifier):
                    outside += 1
                    continue
                safe_results.append({'id': identifier, 'path': path, 'score': item.get('score')})
            expected_path = prefix + 'expediente-a/' + expected
            rank = next((i + 1 for i, item in enumerate(results) if item.get('id') in owned and owned[item['id']] == expected_path), None)
            report['queries'].append({'query': query, 'expected_path': expected_path, 'results': safe_results,
                'http_status': status, 'seconds': seconds, 'degraded': body.get('degraded'),
                'first_hit_rank': rank, 'unowned_result_count': outside,
                'filter_pass': outside == 0 and all(item['path'].startswith(prefix + 'expediente-a/') for item in safe_results)})
        count = len(queries)
        report['recall_at_3'] = sum(q['first_hit_rank'] is not None for q in report['queries']) / count
        report['mrr_at_3'] = sum(1 / q['first_hit_rank'] for q in report['queries'] if q['first_hit_rank']) / count
        # Compare both public retrieval routes on the same owned corpus. Never
        # relax the path boundary or record unrelated result contents.
        for route in ['/v1/memories/search', '/memory/search']:
            status, body, seconds = call(route, {'query': '12500', 'limit': 3,
                'mode': 'snippet', 'filters': {'path_prefix': prefix + 'expediente-a/'}})
            results = body.get('results', [])
            report['controls'].append({'route': route, 'query': '12500',
                'http_status': status, 'seconds': seconds, 'degraded': body.get('degraded'),
                'owned_ids': [item.get('id') for item in results if item.get('id') in owned],
                'unowned_result_count': sum(item.get('id') not in owned for item in results),
                'expected_found': any(owned.get(item.get('id')) == prefix + 'expediente-a/pago' for item in results)})
    except Exception as error:
        # Never serialize server response bodies or exception strings containing data.
        report['errors'].append(type(error).__name__)
    finally:
        for identifier, path in owned.items():
            try:
                status, body, seconds = call('/memory/delete', {'id': identifier})
                get_status, get_body, _ = call('/v1/memories/' + urllib.parse.quote(identifier, safe=''))
                report['cleanup'].append({'id': identifier, 'path': path, 'http_status': status,
                    'status': body.get('status'), 'seconds': seconds, 'read_after_delete_status': get_status,
                    'absent': get_status == 404 or (get_body.get('memory') is None and get_body.get('status') == 'error')})
            except Exception as error:
                report['cleanup'].append({'id': identifier, 'path': path, 'error_type': type(error).__name__, 'absent': False})
        args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'artifact': str(args.output), 'recall_at_3': report.get('recall_at_3'),
        'mrr_at_3': report.get('mrr_at_3'), 'created': len(owned),
        'cleanup_absent': sum(item.get('absent', False) for item in report['cleanup']), 'errors': report['errors']}))
    return int(bool(report['errors']) or report.get('recall_at_3') != 1.0
        or not all(item.get('filter_pass') and item.get('http_status') == 200 for item in report['queries'])
        or not all(item.get('absent') for item in report['cleanup']))


if __name__ == '__main__':
    raise SystemExit(main())
