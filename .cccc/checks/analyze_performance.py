#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
CCCC Performance Analyzer - Foreman Self-Optimization Inspection Tool

Extracts performance metrics from ledger.jsonl, identifies problem patterns, and generates optimization suggestions.
For Foreman to call during inspections.

Usage:
    python .cccc/checks/analyze_performance.py [--last N] [--output PATH]
"""

import json
import re
import sys
from collections import defaultdict
from datetime import datetime
from pathlib import Path
from typing import Dict, List, Any, Optional


def parse_ts(ts_str: str) -> Optional[datetime]:
    """Parse timestamp string"""
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%SZ", "%Y-%m-%dT%H:%M:%S"]:
        try:
            return datetime.strptime(ts_str, fmt)
        except ValueError:
            continue
    return None


def load_ledger(ledger_path: Path, last_n: int = 500) -> List[Dict]:
    """Load recent N ledger records"""
    if not ledger_path.exists():
        return []

    lines = []
    try:
        with open(ledger_path, 'r', encoding='utf-8', errors='replace') as f:
            lines = f.readlines()
    except Exception:
        return []

    records = []
    for line in lines[-last_n:]:
        line = line.strip()
        if not line:
            continue
        try:
            records.append(json.loads(line))
        except json.JSONDecodeError:
            continue

    return records


def analyze_encoding_issues(records: List[Dict]) -> Dict:
    """Analyze encoding issues"""
    issues = []
    for r in records:
        if r.get('kind') == 'mailbox-diag':
            encoding = r.get('encoding', '')
            if 'latin1' in encoding or 'ignore' in encoding:
                issues.append({
                    'ts': r.get('ts'),
                    'file': r.get('file', ''),
                    'encoding': encoding,
                    'nul_ratio': r.get('nul_ratio', 0)
                })

    return {
        'count': len(issues),
        'severity': 'high' if len(issues) > 5 else 'medium' if len(issues) > 0 else 'low',
        'details': issues[-5:],  # Latest 5
        'suggestion': 'Check Agent output encoding, ensure UTF-8 is used' if issues else None
    }


def analyze_communication_efficiency(records: List[Dict]) -> Dict:
    """Analyze communication efficiency"""
    handoffs = {}  # mid -> {sent_ts, ack_ts}
    ack_timeouts = 0
    total_handoffs = 0
    latencies = []

    for r in records:
        kind = r.get('kind', '')
        mid = r.get('mid', '')

        if kind == 'handoff' and mid:
            total_handoffs += 1
            ts = parse_ts(r.get('ts', ''))
            if ts:
                handoffs[mid] = {'sent_ts': ts, 'to': r.get('to')}

        elif kind == 'ack-file':
            # Reverse lookup by seq
            seq = r.get('seq', '')
            ts = parse_ts(r.get('ts', ''))
            if ts:
                # Find matching handoff
                for m, v in handoffs.items():
                    if seq in m and 'ack_ts' not in v:
                        v['ack_ts'] = ts
                        latency = (ts - v['sent_ts']).total_seconds()
                        latencies.append(latency)
                        break

    # Calculate timeouts (>40s considered timeout)
    for mid, v in handoffs.items():
        if 'ack_ts' not in v:
            ack_timeouts += 1

    avg_latency = sum(latencies) / len(latencies) if latencies else 0
    timeout_rate = ack_timeouts / total_handoffs if total_handoffs > 0 else 0

    return {
        'total_handoffs': total_handoffs,
        'avg_latency_seconds': round(avg_latency, 2),
        'ack_timeout_count': ack_timeouts,
        'ack_timeout_rate': round(timeout_rate, 3),
        'severity': 'high' if timeout_rate > 0.2 else 'medium' if timeout_rate > 0.1 else 'low',
        'suggestion': f'ACK timeout rate {timeout_rate:.1%}, suggest adjusting ack_timeout_seconds' if timeout_rate > 0.1 else None
    }


def analyze_collaboration_patterns(records: List[Dict]) -> Dict:
    """Analyze collaboration patterns"""
    asks = []
    risks = []
    unanswered_asks = []
    unhandled_risks = []

    ask_tags = set()

    for r in records:
        kind = r.get('kind', '')
        tag = r.get('tag', '')

        if kind == 'event-ask':
            asks.append({
                'ts': r.get('ts'),
                'tag': tag,
                'to': r.get('to'),
                'text': r.get('text', '')[:100]
            })
            ask_tags.add(tag)

        elif kind == 'event-risk':
            risks.append({
                'ts': r.get('ts'),
                'tag': tag,
                'sev': r.get('sev', 'unknown'),
                'text': r.get('text', '')[:100]
            })

        # Check if progress/evidence responded to ask
        elif kind in ('event-progress', 'event-evidence') and tag in ask_tags:
            ask_tags.discard(tag)

    # Unanswered asks
    for a in asks:
        if a['tag'] in ask_tags:
            unanswered_asks.append(a)

    # Unhandled high risks
    for risk in risks:
        if risk['sev'] in ('high', 'med'):
            unhandled_risks.append(risk)

    return {
        'total_asks': len(asks),
        'unanswered_asks': len(unanswered_asks),
        'total_risks': len(risks),
        'high_med_risks': len([r for r in risks if r['sev'] in ('high', 'med')]),
        'severity': 'high' if len(unanswered_asks) > 3 else 'medium' if len(unanswered_asks) > 0 else 'low',
        'unanswered_details': unanswered_asks[-3:],
        'risk_details': unhandled_risks[-3:],
        'suggestion': f'There are {len(unanswered_asks)} unanswered Asks, suggest reviewing collaboration process' if unanswered_asks else None
    }


def analyze_system_health(records: List[Dict]) -> Dict:
    """Analyze system health status"""
    crashes = []
    errors = []
    restarts = []

    for r in records:
        kind = r.get('kind', '')

        if 'crash' in kind:
            crashes.append({
                'ts': r.get('ts'),
                'peer': r.get('peer'),
                'error': r.get('error', '')[:100]
            })

        elif 'error' in kind:
            errors.append({
                'ts': r.get('ts'),
                'kind': kind,
                'reason': r.get('reason', '')
            })

        elif 'restart' in kind:
            restarts.append({
                'ts': r.get('ts'),
                'peer': r.get('peer'),
                'reason': r.get('reason', '')
            })

    return {
        'crash_count': len(crashes),
        'error_count': len(errors),
        'restart_count': len(restarts),
        'severity': 'high' if crashes else 'medium' if errors else 'low',
        'crashes': crashes[-3:],
        'errors': errors[-3:],
        'suggestion': f'Detected {len(crashes)} crashes, need to investigate causes' if crashes else None
    }


def analyze_message_quality(records: List[Dict]) -> Dict:
    """Analyze message quality"""
    forwards = 0
    drops = 0
    low_signal = 0

    for r in records:
        kind = r.get('kind', '')

        if kind == 'to_peer-forward':
            forwards += 1
        elif kind == 'to_peer-drop':
            drops += 1
            if 'low-signal' in str(r.get('reason', '')):
                low_signal += 1

    total = forwards + drops
    drop_rate = drops / total if total > 0 else 0

    return {
        'total_messages': total,
        'forwarded': forwards,
        'dropped': drops,
        'low_signal_drops': low_signal,
        'drop_rate': round(drop_rate, 3),
        'severity': 'high' if drop_rate > 0.3 else 'medium' if drop_rate > 0.1 else 'low',
        'suggestion': f'Message drop rate {drop_rate:.1%}, cooldown settings may be too strict' if drop_rate > 0.2 else None
    }


def generate_optimization_proposals(analysis: Dict) -> List[Dict]:
    """Generate optimization proposals"""
    proposals = []

    # Encoding issues
    if analysis['encoding']['severity'] != 'low':
        proposals.append({
            'target': 'Agent Behavior',
            'issue': 'Message encoding anomalies',
            'action': 'Ensure to_peer.md/to_user.md are written in UTF-8 encoding',
            'priority': 'high' if analysis['encoding']['severity'] == 'high' else 'medium'
        })

    # Communication efficiency
    comm = analysis['communication']
    if comm['ack_timeout_rate'] > 0.15:
        proposals.append({
            'target': 'cli_profiles.yaml',
            'param': 'delivery.ack_timeout_seconds',
            'current': 40,
            'suggested': 60,
            'issue': f"ACK timeout rate {comm['ack_timeout_rate']:.1%}",
            'priority': 'high'
        })

    if comm['avg_latency_seconds'] > 30:
        proposals.append({
            'target': 'cli_profiles.yaml',
            'param': 'delivery.paste_max_wait_seconds',
            'current': 6,
            'suggested': 10,
            'issue': f"Average latency {comm['avg_latency_seconds']}s",
            'priority': 'medium'
        })

    # Collaboration patterns
    collab = analysis['collaboration']
    if collab['unanswered_asks'] > 2:
        proposals.append({
            'target': 'Agent Collaboration',
            'issue': f"{collab['unanswered_asks']} unanswered Asks",
            'action': 'Check PEERA.md/PEERB.md rules, emphasize Asks must be answered',
            'priority': 'high'
        })

    # Message quality
    quality = analysis['message_quality']
    if quality['drop_rate'] > 0.25:
        proposals.append({
            'target': 'policies.yaml',
            'param': 'handoff_filter.cooldown_seconds',
            'current': 15,
            'suggested': 10,
            'issue': f"Message drop rate {quality['drop_rate']:.1%}",
            'priority': 'medium'
        })

    return proposals


def format_report(analysis: Dict, proposals: List[Dict]) -> str:
    """Format diagnostic report"""
    lines = [
        "# CCCC Performance Diagnostic Report",
        f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}",
        "",
        "## 1. Encoding Quality",
        f"- Issues: {analysis['encoding']['count']}",
        f"- Severity: {analysis['encoding']['severity']}",
    ]

    if analysis['encoding']['suggestion']:
        lines.append(f"- Suggestion: {analysis['encoding']['suggestion']}")

    lines.extend([
        "",
        "## 2. Communication Efficiency",
        f"- Total Handoffs: {analysis['communication']['total_handoffs']}",
        f"- Average Latency: {analysis['communication']['avg_latency_seconds']}s",
        f"- ACK Timeout Rate: {analysis['communication']['ack_timeout_rate']:.1%}",
        f"- Severity: {analysis['communication']['severity']}",
    ])

    if analysis['communication']['suggestion']:
        lines.append(f"- Suggestion: {analysis['communication']['suggestion']}")

    lines.extend([
        "",
        "## 3. Collaboration Patterns",
        f"- Total Asks: {analysis['collaboration']['total_asks']}",
        f"- Unanswered Asks: {analysis['collaboration']['unanswered_asks']}",
        f"- High/Med Risks: {analysis['collaboration']['high_med_risks']}",
        f"- Severity: {analysis['collaboration']['severity']}",
    ])

    if analysis['collaboration']['suggestion']:
        lines.append(f"- Suggestion: {analysis['collaboration']['suggestion']}")

    lines.extend([
        "",
        "## 4. Message Quality",
        f"- Total Messages: {analysis['message_quality']['total_messages']}",
        f"- Forwarded: {analysis['message_quality']['forwarded']}",
        f"- Dropped: {analysis['message_quality']['dropped']} (Low signal: {analysis['message_quality']['low_signal_drops']})",
        f"- Drop Rate: {analysis['message_quality']['drop_rate']:.1%}",
        f"- Severity: {analysis['message_quality']['severity']}",
    ])

    if analysis['message_quality']['suggestion']:
        lines.append(f"- Suggestion: {analysis['message_quality']['suggestion']}")

    lines.extend([
        "",
        "## 5. System Health",
        f"- Crashes: {analysis['system_health']['crash_count']}",
        f"- Errors: {analysis['system_health']['error_count']}",
        f"- Restarts: {analysis['system_health']['restart_count']}",
        f"- Severity: {analysis['system_health']['severity']}",
    ])

    if analysis['system_health']['suggestion']:
        lines.append(f"- Suggestion: {analysis['system_health']['suggestion']}")

    if proposals:
        lines.extend([
            "",
            "## 6. Optimization Proposals",
            ""
        ])
        for i, p in enumerate(proposals, 1):
            lines.append(f"### Proposal {i} [{p.get('priority', 'medium')}]")
            lines.append(f"- Target: {p.get('target')}")
            lines.append(f"- Issue: {p.get('issue')}")
            if 'param' in p:
                lines.append(f"- Parameter: {p['param']}")
                lines.append(f"- Current Value: {p.get('current')}")
                lines.append(f"- Suggested Value: {p.get('suggested')}")
            if 'action' in p:
                lines.append(f"- Action: {p['action']}")
            lines.append("")

    return "\n".join(lines)


def format_peer_directive(analysis: Dict, proposals: List[Dict]) -> str:
    """Format directive for Peer"""
    high_priority = [p for p in proposals if p.get('priority') == 'high']

    if not high_priority and all(a['severity'] == 'low' for a in analysis.values() if isinstance(a, dict) and 'severity' in a):
        return ""

    lines = [
        "To: Both",
        "<TO_PEER>",
        "",
        "## [Foreman Self-Optimization Inspection Results]",
        "",
    ]

    # High priority issues
    if high_priority:
        lines.append("### Issues Requiring Immediate Attention:")
        for p in high_priority:
            lines.append(f"- **{p.get('target')}**: {p.get('issue')}")
            if 'action' in p:
                lines.append(f"  - Suggestion: {p['action']}")
        lines.append("")

    # Communication efficiency reminder
    comm = analysis['communication']
    if comm['severity'] != 'low':
        lines.append(f"### Communication Efficiency Reminder")
        lines.append(f"- ACK timeout rate: {comm['ack_timeout_rate']:.1%}")
        lines.append(f"- Please ensure timely processing of inbox messages")
        lines.append("")

    # Unanswered Asks
    collab = analysis['collaboration']
    if collab['unanswered_details']:
        lines.append("### Asks Awaiting Response:")
        for ask in collab['unanswered_details']:
            lines.append(f"- [{ask['tag']}] to {ask['to']}: {ask['text'][:60]}...")
        lines.append("")

    lines.extend([
        "Full report at: `.cccc/work/foreman/diagnosis/latest.md`",
        "",
        "</TO_PEER>"
    ])

    return "\n".join(lines)


def main():
    import argparse
    parser = argparse.ArgumentParser(description='CCCC Performance Analyzer')
    parser.add_argument('--last', type=int, default=500, help='Analyze recent N records')
    parser.add_argument('--output', type=str, help='Output directory')
    parser.add_argument('--json', action='store_true', help='Output in JSON format')
    args = parser.parse_args()

    # Find project root
    cwd = Path.cwd()
    home = cwd / '.cccc'
    if not home.exists():
        print("Error: .cccc directory not found", file=sys.stderr)
        sys.exit(1)

    ledger_path = home / 'state' / 'ledger.jsonl'
    records = load_ledger(ledger_path, args.last)

    if not records:
        print("Warning: ledger is empty or does not exist", file=sys.stderr)
        sys.exit(0)

    # Execute analysis
    analysis = {
        'encoding': analyze_encoding_issues(records),
        'communication': analyze_communication_efficiency(records),
        'collaboration': analyze_collaboration_patterns(records),
        'message_quality': analyze_message_quality(records),
        'system_health': analyze_system_health(records),
    }

    proposals = generate_optimization_proposals(analysis)

    if args.json:
        print(json.dumps({'analysis': analysis, 'proposals': proposals}, indent=2, ensure_ascii=False))
    else:
        report = format_report(analysis, proposals)
        print(report)

        # Save report
        output_dir = Path(args.output) if args.output else home / 'work' / 'foreman' / 'diagnosis'
        output_dir.mkdir(parents=True, exist_ok=True)

        report_path = output_dir / 'latest.md'
        report_path.write_text(report, encoding='utf-8')
        print(f"\nReport saved to: {report_path}")

        # Generate directive for Peer
        directive = format_peer_directive(analysis, proposals)
        if directive:
            directive_path = output_dir / 'peer_directive.md'
            directive_path.write_text(directive, encoding='utf-8')
            print(f"Peer directive saved to: {directive_path}")


if __name__ == '__main__':
    main()
