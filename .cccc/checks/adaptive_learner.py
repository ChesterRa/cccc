#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
CCCC Adaptive Learner

Learns project characteristics from historical data and dynamically adjusts thresholds and judgment rules.
Core idea: Use the project's own statistical distribution to define "normal" and "abnormal".

Usage:
    python .cccc/checks/adaptive_learner.py learn     # Learn and save baseline
    python .cccc/checks/adaptive_learner.py analyze   # Analyze current state based on baseline
    python .cccc/checks/adaptive_learner.py status    # Show learning status
"""

import json
import math
import sys
from collections import defaultdict
from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, List, Any, Optional, Tuple
from dataclasses import dataclass, asdict


@dataclass
class Distribution:
    """Statistical distribution"""
    count: int = 0
    mean: float = 0.0
    std: float = 0.0
    min_val: float = float('inf')
    max_val: float = float('-inf')
    p50: float = 0.0  # Median
    p90: float = 0.0  # 90th percentile
    p95: float = 0.0  # 95th percentile

    def anomaly_threshold(self, sensitivity: float = 2.0) -> float:
        """Calculate anomaly threshold (mean + N * std)"""
        return self.mean + sensitivity * self.std

    def is_anomaly(self, value: float, sensitivity: float = 2.0) -> bool:
        """Determine if value is anomalous"""
        if self.count < 10:  # Insufficient samples, no judgment
            return False
        return value > self.anomaly_threshold(sensitivity)


@dataclass
class ProjectBaseline:
    """Project baseline - characteristics learned from historical data"""
    # Metadata
    project_id: str = ""
    learned_at: str = ""
    sample_count: int = 0
    time_span_hours: float = 0.0

    # Communication characteristics
    handoff_latency: Distribution = None  # handoff -> ack latency distribution
    message_length: Distribution = None   # Message length distribution
    handoff_interval: Distribution = None # Handoff interval distribution

    # Collaboration characteristics
    ask_response_rate: float = 0.0        # Ratio of Asks that are answered
    avg_asks_per_session: float = 0.0     # Average Ask count per session
    risk_acknowledge_rate: float = 0.0    # Ratio of Risks that are handled

    # Activity characteristics
    peer_activity_ratio: Dict[str, float] = None  # Activity percentage for each Peer
    event_type_distribution: Dict[str, float] = None  # Event type distribution

    # Error characteristics
    encoding_error_rate: float = 0.0      # Encoding error rate
    crash_rate_per_hour: float = 0.0      # Crash rate per hour

    def __post_init__(self):
        if self.handoff_latency is None:
            self.handoff_latency = Distribution()
        if self.message_length is None:
            self.message_length = Distribution()
        if self.handoff_interval is None:
            self.handoff_interval = Distribution()
        if self.peer_activity_ratio is None:
            self.peer_activity_ratio = {}
        if self.event_type_distribution is None:
            self.event_type_distribution = {}


def parse_ts(ts_str: str) -> Optional[datetime]:
    """Parse timestamp"""
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%SZ"]:
        try:
            return datetime.strptime(ts_str, fmt)
        except ValueError:
            continue
    return None


def load_ledger(ledger_path: Path, max_records: int = 5000) -> List[Dict]:
    """Load ledger records"""
    if not ledger_path.exists():
        return []

    records = []
    try:
        with open(ledger_path, 'r', encoding='utf-8', errors='replace') as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    records.append(json.loads(line))
                except json.JSONDecodeError:
                    continue
    except Exception:
        return []

    return records[-max_records:]


def compute_distribution(values: List[float]) -> Distribution:
    """Calculate statistical distribution"""
    if not values:
        return Distribution()

    n = len(values)
    sorted_vals = sorted(values)

    mean = sum(values) / n
    variance = sum((x - mean) ** 2 for x in values) / n if n > 1 else 0
    std = math.sqrt(variance)

    return Distribution(
        count=n,
        mean=round(mean, 3),
        std=round(std, 3),
        min_val=round(min(values), 3),
        max_val=round(max(values), 3),
        p50=round(sorted_vals[n // 2], 3),
        p90=round(sorted_vals[int(n * 0.9)], 3) if n >= 10 else round(sorted_vals[-1], 3),
        p95=round(sorted_vals[int(n * 0.95)], 3) if n >= 20 else round(sorted_vals[-1], 3),
    )


def learn_baseline(records: List[Dict]) -> ProjectBaseline:
    """Learn project baseline from historical data"""
    baseline = ProjectBaseline()

    if not records:
        return baseline

    # Time range
    timestamps = []
    for r in records:
        ts = parse_ts(r.get('ts', ''))
        if ts:
            timestamps.append(ts)

    if timestamps:
        time_span = (max(timestamps) - min(timestamps)).total_seconds() / 3600
        baseline.time_span_hours = round(time_span, 2)

    baseline.sample_count = len(records)
    baseline.learned_at = datetime.now().strftime("%Y-%m-%d %H:%M:%S")

    # 1. Learn handoff latency distribution
    handoffs = {}  # mid -> sent_ts
    latencies = []

    for r in records:
        kind = r.get('kind', '')
        if kind == 'handoff':
            mid = r.get('mid', '')
            ts = parse_ts(r.get('ts', ''))
            if mid and ts:
                handoffs[mid] = ts
        elif kind == 'ack-file':
            # Extract mid from filename
            filename = r.get('file', '')
            ts = parse_ts(r.get('ts', ''))
            if ts:
                for mid, sent_ts in handoffs.items():
                    if mid[:20] in filename:  # mid partial match
                        latency = (ts - sent_ts).total_seconds()
                        if 0 < latency < 300:  # Reasonable range
                            latencies.append(latency)
                        break

    baseline.handoff_latency = compute_distribution(latencies)

    # 2. Learn message length distribution
    message_lengths = []
    for r in records:
        chars = r.get('chars')
        if chars and isinstance(chars, (int, float)) and chars > 0:
            message_lengths.append(chars)

    baseline.message_length = compute_distribution(message_lengths)

    # 3. Learn handoff interval distribution
    handoff_times = []
    for r in records:
        if r.get('kind') == 'handoff':
            ts = parse_ts(r.get('ts', ''))
            if ts:
                handoff_times.append(ts)

    handoff_times.sort()
    intervals = []
    for i in range(1, len(handoff_times)):
        interval = (handoff_times[i] - handoff_times[i-1]).total_seconds()
        if 0 < interval < 3600:  # Within 1 hour
            intervals.append(interval)

    baseline.handoff_interval = compute_distribution(intervals)

    # 4. Learn Ask response rate
    ask_tags = set()
    responded_tags = set()

    for r in records:
        kind = r.get('kind', '')
        tag = r.get('tag', '')

        if kind == 'event-ask' and tag:
            ask_tags.add(tag)
        elif kind in ('event-progress', 'event-evidence') and tag in ask_tags:
            responded_tags.add(tag)

    if ask_tags:
        baseline.ask_response_rate = round(len(responded_tags) / len(ask_tags), 3)

    # 5. Learn Risk handling rate
    risk_tags = set()
    handled_risks = set()

    for r in records:
        kind = r.get('kind', '')
        tag = r.get('tag', '')

        if kind == 'event-risk' and tag:
            risk_tags.add(tag)
        elif kind == 'event-evidence' and tag in risk_tags:
            handled_risks.add(tag)

    if risk_tags:
        baseline.risk_acknowledge_rate = round(len(handled_risks) / len(risk_tags), 3)

    # 6. Learn Peer activity distribution
    peer_activity = defaultdict(int)
    for r in records:
        from_peer = r.get('from', '')
        if from_peer in ('PeerA', 'PeerB'):
            peer_activity[from_peer] += 1

    total_activity = sum(peer_activity.values())
    if total_activity > 0:
        baseline.peer_activity_ratio = {
            k: round(v / total_activity, 3)
            for k, v in peer_activity.items()
        }

    # 7. Learn event type distribution
    event_counts = defaultdict(int)
    for r in records:
        kind = r.get('kind', '')
        if kind:
            # Simplify event type
            simple_kind = kind.split('-')[0] if '-' in kind else kind
            event_counts[simple_kind] += 1

    total_events = sum(event_counts.values())
    if total_events > 0:
        baseline.event_type_distribution = {
            k: round(v / total_events, 3)
            for k, v in sorted(event_counts.items(), key=lambda x: -x[1])[:10]
        }

    # 8. Learn encoding error rate
    encoding_errors = sum(1 for r in records if r.get('kind') == 'mailbox-diag' and 'latin1' in str(r.get('encoding', '')))
    total_mailbox_ops = sum(1 for r in records if 'mailbox' in r.get('kind', '') or 'to_peer' in r.get('kind', ''))

    if total_mailbox_ops > 0:
        baseline.encoding_error_rate = round(encoding_errors / total_mailbox_ops, 4)

    # 9. Learn crash rate
    crashes = sum(1 for r in records if 'crash' in r.get('kind', ''))
    if baseline.time_span_hours > 0:
        baseline.crash_rate_per_hour = round(crashes / baseline.time_span_hours, 4)

    return baseline


def analyze_with_baseline(records: List[Dict], baseline: ProjectBaseline) -> Dict:
    """Analyze current state based on baseline"""

    # Calculate current metrics
    current = learn_baseline(records[-100:])  # Latest 100 records

    anomalies = []
    insights = []

    # 1. Check latency anomalies
    if baseline.handoff_latency.count >= 10 and current.handoff_latency.count >= 5:
        if current.handoff_latency.mean > baseline.handoff_latency.anomaly_threshold(2.0):
            anomalies.append({
                'type': 'latency_spike',
                'severity': 'high',
                'message': f"Current average latency {current.handoff_latency.mean}s significantly higher than baseline {baseline.handoff_latency.mean}s",
                'baseline': baseline.handoff_latency.mean,
                'current': current.handoff_latency.mean,
                'threshold': baseline.handoff_latency.anomaly_threshold(2.0)
            })

    # 2. Check message length anomalies
    if baseline.message_length.count >= 10 and current.message_length.count >= 5:
        # Short messages may indicate low-quality interactions
        if current.message_length.mean < baseline.message_length.mean * 0.5:
            anomalies.append({
                'type': 'short_messages',
                'severity': 'medium',
                'message': f"Current average message length {current.message_length.mean} is only {current.message_length.mean/baseline.message_length.mean:.0%}",
                'suggestion': 'Check for large number of low-signal messages'
            })
        # Long messages may indicate excessive communication
        elif current.message_length.mean > baseline.message_length.p95:
            insights.append({
                'type': 'long_messages',
                'message': f"Current messages are long, may affect processing efficiency"
            })

    # 3. Check for declining Ask response rate
    if baseline.ask_response_rate > 0 and current.ask_response_rate < baseline.ask_response_rate * 0.7:
        anomalies.append({
            'type': 'low_ask_response',
            'severity': 'high',
            'message': f"Ask response rate decreased from {baseline.ask_response_rate:.0%} to {current.ask_response_rate:.0%}",
            'suggestion': 'Check collaboration process, ensure Asks are responded to promptly'
        })

    # 4. Check for imbalanced Peer activity
    if baseline.peer_activity_ratio and current.peer_activity_ratio:
        for peer, baseline_ratio in baseline.peer_activity_ratio.items():
            current_ratio = current.peer_activity_ratio.get(peer, 0)
            if baseline_ratio > 0.2 and current_ratio < baseline_ratio * 0.3:
                anomalies.append({
                    'type': 'peer_inactive',
                    'severity': 'medium',
                    'message': f"{peer} activity significantly decreased（{baseline_ratio:.0%} → {current_ratio:.0%}）",
                    'suggestion': f'检查 {peer} is functioning normally'
                })

    # 5. Check for rising encoding error rate
    if current.encoding_error_rate > baseline.encoding_error_rate * 2 and current.encoding_error_rate > 0.01:
        anomalies.append({
            'type': 'encoding_errors',
            'severity': 'medium',
            'message': f"Encoding error rate increased: {baseline.encoding_error_rate:.2%} → {current.encoding_error_rate:.2%}",
            'suggestion': '检查 Agent 输出编码设置'
        })

    # 6. Generate adaptive threshold suggestions
    adaptive_thresholds = {
        'ack_timeout_seconds': {
            'suggested': max(30, round(baseline.handoff_latency.p95 * 1.5)),
            'reason': f"Based on P95 latency {baseline.handoff_latency.p95}s calculated"
        },
        'cooldown_seconds': {
            'suggested': max(5, round(baseline.handoff_interval.p50 * 0.3)),
            'reason': f"Based on median interval {baseline.handoff_interval.p50}s calculated"
        },
        'min_chars': {
            'suggested': max(20, round(baseline.message_length.p50 * 0.1)),
            'reason': f"Based on median message length {baseline.message_length.p50} calculated"
        }
    }

    # 7. Project characteristic insights
    if baseline.time_span_hours >= 1:
        handoffs_per_hour = baseline.sample_count / baseline.time_span_hours

        if handoffs_per_hour > 50:
            insights.append({
                'type': 'high_activity',
                'message': f'Project has frequent interactions（{handoffs_per_hour:.0f} events/hour），suggest optimizing message filtering'
            })
        elif handoffs_per_hour < 5:
            insights.append({
                'type': 'low_activity',
                'message': f'Project has few interactions（{handoffs_per_hour:.1f} events/hour），current configuration may be too conservative'
            })

    return {
        'baseline_summary': {
            'learned_at': baseline.learned_at,
            'sample_count': baseline.sample_count,
            'time_span_hours': baseline.time_span_hours,
            'avg_latency': baseline.handoff_latency.mean,
            'avg_message_length': baseline.message_length.mean,
            'ask_response_rate': baseline.ask_response_rate,
        },
        'current_summary': {
            'sample_count': current.sample_count,
            'avg_latency': current.handoff_latency.mean,
            'avg_message_length': current.message_length.mean,
            'ask_response_rate': current.ask_response_rate,
        },
        'anomalies': anomalies,
        'insights': insights,
        'adaptive_thresholds': adaptive_thresholds,
    }


def save_baseline(baseline: ProjectBaseline, path: Path):
    """Save baseline to file"""
    data = {
        'project_id': baseline.project_id,
        'learned_at': baseline.learned_at,
        'sample_count': baseline.sample_count,
        'time_span_hours': baseline.time_span_hours,
        'handoff_latency': asdict(baseline.handoff_latency),
        'message_length': asdict(baseline.message_length),
        'handoff_interval': asdict(baseline.handoff_interval),
        'ask_response_rate': baseline.ask_response_rate,
        'risk_acknowledge_rate': baseline.risk_acknowledge_rate,
        'peer_activity_ratio': baseline.peer_activity_ratio,
        'event_type_distribution': baseline.event_type_distribution,
        'encoding_error_rate': baseline.encoding_error_rate,
        'crash_rate_per_hour': baseline.crash_rate_per_hour,
    }

    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(data, f, indent=2, ensure_ascii=False)


def load_baseline(path: Path) -> Optional[ProjectBaseline]:
    """Load baseline from file"""
    if not path.exists():
        return None

    try:
        with open(path, 'r', encoding='utf-8') as f:
            data = json.load(f)

        baseline = ProjectBaseline(
            project_id=data.get('project_id', ''),
            learned_at=data.get('learned_at', ''),
            sample_count=data.get('sample_count', 0),
            time_span_hours=data.get('time_span_hours', 0),
            ask_response_rate=data.get('ask_response_rate', 0),
            risk_acknowledge_rate=data.get('risk_acknowledge_rate', 0),
            peer_activity_ratio=data.get('peer_activity_ratio', {}),
            event_type_distribution=data.get('event_type_distribution', {}),
            encoding_error_rate=data.get('encoding_error_rate', 0),
            crash_rate_per_hour=data.get('crash_rate_per_hour', 0),
        )

        if 'handoff_latency' in data:
            baseline.handoff_latency = Distribution(**data['handoff_latency'])
        if 'message_length' in data:
            baseline.message_length = Distribution(**data['message_length'])
        if 'handoff_interval' in data:
            baseline.handoff_interval = Distribution(**data['handoff_interval'])

        return baseline
    except Exception as e:
        print(f"Failed to load baseline: {e}", file=sys.stderr)
        return None


def format_analysis_report(analysis: Dict) -> str:
    """Format analysis report"""
    lines = [
        "# CCCC 自适应分析报告",
        f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}",
        "",
        "## Baseline vs Current",
        "",
        "| Metric | Baseline | Current | Change |",
        "|------|------|------|------|",
    ]

    bs = analysis['baseline_summary']
    cs = analysis['current_summary']

    def change_indicator(baseline, current):
        if baseline == 0:
            return "N/A"
        ratio = current / baseline
        if ratio > 1.2:
            return f"↑ {ratio:.0%}"
        elif ratio < 0.8:
            return f"↓ {ratio:.0%}"
        else:
            return "≈"

    lines.append(f"| Average Latency | {bs['avg_latency']}s | {cs['avg_latency']}s | {change_indicator(bs['avg_latency'], cs['avg_latency'])} |")
    lines.append(f"| Message Length | {bs['avg_message_length']} | {cs['avg_message_length']} | {change_indicator(bs['avg_message_length'], cs['avg_message_length'])} |")
    lines.append(f"| Ask Response Rate | {bs['ask_response_rate']:.0%} | {cs['ask_response_rate']:.0%} | {change_indicator(bs['ask_response_rate'], cs['ask_response_rate'])} |")

    if analysis['anomalies']:
        lines.extend([
            "",
            "## Detected Anomalies",
            ""
        ])
        for a in analysis['anomalies']:
            sev_icon = "🔴" if a['severity'] == 'high' else "🟡"
            lines.append(f"{sev_icon} **{a['type']}**: {a['message']}")
            if 'suggestion' in a:
                lines.append(f"   - Suggestion: {a['suggestion']}")

    if analysis['insights']:
        lines.extend([
            "",
            "## Project Insights",
            ""
        ])
        for i in analysis['insights']:
            lines.append(f"💡 **{i['type']}**: {i['message']}")

    lines.extend([
        "",
        "## Adaptive Threshold Suggestions",
        "",
        "基于项目历史数据calculated的推荐配置：",
        ""
    ])

    for param, info in analysis['adaptive_thresholds'].items():
        lines.append(f"- `{param}`: **{info['suggested']}** ({info['reason']})")

    return "\n".join(lines)


def main():
    if len(sys.argv) < 2:
        print("Usage: python adaptive_learner.py [learn|analyze|status]")
        sys.exit(1)

    command = sys.argv[1]

    # 查找项目路径
    cwd = Path.cwd()
    home = cwd / '.cccc'
    if not home.exists():
        print("Error: .cccc directory not found", file=sys.stderr)
        sys.exit(1)

    ledger_path = home / 'state' / 'ledger.jsonl'
    baseline_path = home / 'state' / 'project_baseline.json'

    if command == 'learn':
        print("正在从历史数据学习项目Baseline...")
        records = load_ledger(ledger_path, max_records=5000)

        if len(records) < 50:
            print(f"Warning: Insufficient samples ({len(records)} records), suggest accumulating more data before learning")

        baseline = learn_baseline(records)
        baseline.project_id = cwd.name

        save_baseline(baseline, baseline_path)

        print(f"\n✅ Baseline学习完成")
        print(f"   - Samples: {baseline.sample_count}")
        print(f"   - Time span: {baseline.time_span_hours:.1f} hours")
        print(f"   - Average Latency: {baseline.handoff_latency.mean}s (P95: {baseline.handoff_latency.p95}s)")
        print(f"   - Message Length: {baseline.message_length.mean} (P50: {baseline.message_length.p50})")
        print(f"   - Ask Response Rate: {baseline.ask_response_rate:.0%}")
        print(f"\nBaseline已保存到: {baseline_path}")

    elif command == 'analyze':
        baseline = load_baseline(baseline_path)

        if baseline is None:
            print("错误: 未找到Baseline，请先Run 'learn' command")
            sys.exit(1)

        records = load_ledger(ledger_path, max_records=500)
        analysis = analyze_with_baseline(records, baseline)

        report = format_analysis_report(analysis)
        print(report)

        # 保存报告
        output_dir = home / 'work' / 'foreman' / 'diagnosis'
        output_dir.mkdir(parents=True, exist_ok=True)

        report_path = output_dir / 'adaptive_analysis.md'
        report_path.write_text(report, encoding='utf-8')

        # 如果有异常，生成 Peer 指令
        if analysis['anomalies']:
            high_anomalies = [a for a in analysis['anomalies'] if a['severity'] == 'high']
            if high_anomalies:
                directive_lines = [
                    "To: Both",
                    "<TO_PEER>",
                    "",
                    "## [Adaptive Analysis] Detected Anomalies",
                    ""
                ]
                for a in high_anomalies:
                    directive_lines.append(f"⚠️ **{a['type']}**: {a['message']}")
                    if 'suggestion' in a:
                        directive_lines.append(f"   Suggestion: {a['suggestion']}")

                directive_lines.extend([
                    "",
                    f"完整报告: `.cccc/work/foreman/diagnosis/adaptive_analysis.md`",
                    "",
                    "</TO_PEER>"
                ])

                directive_path = output_dir / 'adaptive_directive.md'
                directive_path.write_text("\n".join(directive_lines), encoding='utf-8')
                print(f"\nPeer directive generated: {directive_path}")

    elif command == 'status':
        baseline = load_baseline(baseline_path)

        if baseline is None:
            print("Status: Not learned")
            print("Run 'python adaptive_learner.py learn' to start learning")
        else:
            print("Status: Learned")
            print(f"   - Learned at: {baseline.learned_at}")
            print(f"   - Samples: {baseline.sample_count}")
            print(f"   - Project: {baseline.project_id}")

            # 检查Baseline是否过时
            try:
                learned_at = datetime.strptime(baseline.learned_at, "%Y-%m-%d %H:%M:%S")
                age_days = (datetime.now() - learned_at).days
                if age_days > 7:
                    print(f"\n⚠️ Baseline已 {age_days} days, suggest re-learning")
            except:
                pass

    else:
        print(f"未知command: {command}")
        print("可用command: learn, analyze, status")
        sys.exit(1)


if __name__ == '__main__':
    main()
