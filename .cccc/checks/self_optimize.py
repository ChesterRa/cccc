#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
CCCC Self-Optimization Entry Script

Integrates adaptive learning and performance analysis to execute a complete self-optimization inspection workflow.

Usage:
    python .cccc/checks/self_optimize.py              # Full inspection
    python .cccc/checks/self_optimize.py --quick      # Quick inspection (skip learning)
    python .cccc/checks/self_optimize.py --apply      # Inspection and auto-apply suggestions
"""

import json
import sys
from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, Any, List, Optional

# Import other modules
sys.path.insert(0, str(Path(__file__).parent))
from adaptive_learner import (
    load_ledger, learn_baseline, load_baseline, save_baseline,
    analyze_with_baseline, format_analysis_report
)
from analyze_performance import (
    analyze_encoding_issues, analyze_communication_efficiency,
    analyze_collaboration_patterns, analyze_message_quality,
    analyze_system_health, generate_optimization_proposals,
    format_report, format_peer_directive
)


def load_current_config(settings_path: Path) -> Dict[str, Any]:
    """Load current configuration"""
    config = {}

    try:
        import yaml
    except ImportError:
        return config

    # cli_profiles.yaml
    cli_path = settings_path / 'cli_profiles.yaml'
    if cli_path.exists():
        try:
            with open(cli_path, 'r', encoding='utf-8') as f:
                cli_config = yaml.safe_load(f) or {}
                config['delivery'] = cli_config.get('delivery', {})
        except Exception:
            pass

    # policies.yaml
    policies_path = settings_path / 'policies.yaml'
    if policies_path.exists():
        try:
            with open(policies_path, 'r', encoding='utf-8') as f:
                policies_config = yaml.safe_load(f) or {}
                config['handoff_filter'] = policies_config.get('handoff_filter', {})
        except Exception:
            pass

    return config


def compare_thresholds(current: Dict, suggested: Dict) -> List[Dict]:
    """Compare current and suggested configurations"""
    differences = []

    mappings = {
        'ack_timeout_seconds': ('delivery', 'ack_timeout_seconds'),
        'cooldown_seconds': ('handoff_filter', 'cooldown_seconds'),
        'min_chars': ('handoff_filter', 'min_chars'),
    }

    for key, (section, param) in mappings.items():
        if key not in suggested:
            continue

        suggested_val = suggested[key]['suggested']
        current_val = current.get(section, {}).get(param)

        if current_val is None:
            continue

        diff_ratio = abs(suggested_val - current_val) / current_val if current_val > 0 else 0

        if diff_ratio > 0.3:  # Difference exceeds 30%
            differences.append({
                'param': f"{section}.{param}",
                'current': current_val,
                'suggested': suggested_val,
                'diff_ratio': diff_ratio,
                'reason': suggested[key]['reason']
            })

    return differences


def generate_optimization_yaml(differences: List[Dict], output_path: Path):
    """Generate optimization proposals YAML"""
    proposals = {
        'generated_at': datetime.now().strftime('%Y-%m-%d %H:%M:%S'),
        'status': 'pending_approval',
        'proposals': []
    }

    for diff in differences:
        proposals['proposals'].append({
            'param': diff['param'],
            'current': diff['current'],
            'suggested': diff['suggested'],
            'reason': diff['reason'],
            'approved': False
        })

    output_path.parent.mkdir(parents=True, exist_ok=True)

    try:
        import yaml
        with open(output_path, 'w', encoding='utf-8') as f:
            yaml.safe_dump(proposals, f, allow_unicode=True, sort_keys=False)
    except ImportError:
        with open(output_path, 'w', encoding='utf-8') as f:
            json.dump(proposals, f, indent=2, ensure_ascii=False)


def run_self_optimization(home: Path, quick: bool = False, auto_apply: bool = False) -> Dict:
    """Run self-optimization workflow"""
    results = {
        'timestamp': datetime.now().strftime('%Y-%m-%d %H:%M:%S'),
        'baseline_status': 'unknown',
        'adaptive_analysis': None,
        'static_analysis': None,
        'config_differences': [],
        'actions_taken': [],
        'peer_directive': None,
    }

    state_path = home / 'state'
    settings_path = home / 'settings'
    ledger_path = state_path / 'ledger.jsonl'
    baseline_path = state_path / 'project_baseline.json'
    output_dir = home / 'work' / 'foreman' / 'diagnosis'

    # 1. Load ledger
    print("📊 Loading historical data...")
    records = load_ledger(ledger_path, max_records=2000)

    if len(records) < 20:
        print(f"⚠️ Insufficient data ({len(records)} records), skipping analysis")
        results['baseline_status'] = 'insufficient_data'
        return results

    # 2. Check/update baseline
    baseline = load_baseline(baseline_path)

    if baseline is None:
        print("📚 First run, learning project baseline...")
        baseline = learn_baseline(records)
        baseline.project_id = home.parent.name
        save_baseline(baseline, baseline_path)
        results['baseline_status'] = 'newly_learned'
        results['actions_taken'].append('learned_baseline')
    elif not quick:
        # Check if baseline is outdated
        try:
            learned_at = datetime.strptime(baseline.learned_at, "%Y-%m-%d %H:%M:%S")
            age_days = (datetime.now() - learned_at).days

            if age_days > 7:
                print(f"📚 Baseline hasn't been updated for {age_days} days, re-learning...")
                baseline = learn_baseline(records)
                baseline.project_id = home.parent.name
                save_baseline(baseline, baseline_path)
                results['baseline_status'] = 'refreshed'
                results['actions_taken'].append('refreshed_baseline')
            else:
                results['baseline_status'] = 'up_to_date'
        except:
            results['baseline_status'] = 'up_to_date'
    else:
        results['baseline_status'] = 'skipped'

    # 3. Adaptive analysis
    print("🔍 Running adaptive analysis...")
    adaptive_result = analyze_with_baseline(records, baseline)
    results['adaptive_analysis'] = adaptive_result

    # Save adaptive analysis report
    adaptive_report = format_analysis_report(adaptive_result)
    output_dir.mkdir(parents=True, exist_ok=True)
    (output_dir / 'adaptive_analysis.md').write_text(adaptive_report, encoding='utf-8')

    # 4. Static analysis (hard thresholds)
    print("🔍 Running static analysis...")
    static_analysis = {
        'encoding': analyze_encoding_issues(records),
        'communication': analyze_communication_efficiency(records),
        'collaboration': analyze_collaboration_patterns(records),
        'message_quality': analyze_message_quality(records),
        'system_health': analyze_system_health(records),
    }
    static_proposals = generate_optimization_proposals(static_analysis)
    results['static_analysis'] = {
        'analysis': static_analysis,
        'proposals': static_proposals
    }

    # Save static analysis report
    static_report = format_report(static_analysis, static_proposals)
    (output_dir / 'latest.md').write_text(static_report, encoding='utf-8')

    # 5. Compare current configuration with suggestions
    print("⚙️ Comparing configuration differences...")
    current_config = load_current_config(settings_path)
    config_diffs = compare_thresholds(current_config, adaptive_result.get('adaptive_thresholds', {}))
    results['config_differences'] = config_diffs

    if config_diffs:
        print(f"   Found {len(config_diffs)} configuration differences")
        proposal_path = output_dir / 'optimization_proposals.yaml'
        generate_optimization_yaml(config_diffs, proposal_path)
        results['actions_taken'].append('generated_proposals')

    # 6. Generate Peer directive
    all_anomalies = adaptive_result.get('anomalies', [])
    high_priority_static = [p for p in static_proposals if p.get('priority') == 'high']

    if all_anomalies or high_priority_static:
        print("📝 Generating Peer directive...")

        directive_lines = [
            "To: Both",
            "<TO_PEER>",
            "",
            "## [Foreman Self-Optimization Inspection Results]",
            f"Time: {results['timestamp']}",
            "",
        ]

        if all_anomalies:
            directive_lines.append("### Anomalies Found by Adaptive Analysis")
            for a in all_anomalies:
                sev_icon = "🔴" if a['severity'] == 'high' else "🟡"
                directive_lines.append(f"{sev_icon} **{a['type']}**: {a['message']}")
                if 'suggestion' in a:
                    directive_lines.append(f"   - Suggestion: {a['suggestion']}")
            directive_lines.append("")

        if high_priority_static:
            directive_lines.append("### Issues Found by Static Analysis")
            for p in high_priority_static:
                directive_lines.append(f"⚠️ **{p.get('target')}**: {p.get('issue')}")
                if 'action' in p:
                    directive_lines.append(f"   - Suggestion: {p['action']}")
            directive_lines.append("")

        if config_diffs:
            directive_lines.append("### Configuration Optimization Suggestions")
            directive_lines.append("The following configurations do not match project characteristics:")
            for diff in config_diffs[:3]:  # Show at most 3
                directive_lines.append(f"- `{diff['param']}`: Current {diff['current']} → Suggested {diff['suggested']}")
            directive_lines.append(f"\nSee details: `.cccc/work/foreman/diagnosis/optimization_proposals.yaml`")
            directive_lines.append("")

        directive_lines.extend([
            "---",
            "Full reports:",
            "- Adaptive analysis: `.cccc/work/foreman/diagnosis/adaptive_analysis.md`",
            "- Static analysis: `.cccc/work/foreman/diagnosis/latest.md`",
            "",
            "</TO_PEER>"
        ])

        directive = "\n".join(directive_lines)
        directive_path = output_dir / 'peer_directive.md'
        directive_path.write_text(directive, encoding='utf-8')
        results['peer_directive'] = str(directive_path)
        results['actions_taken'].append('generated_peer_directive')

    # 7. Summary
    print("\n" + "=" * 50)
    print("📋 Self-optimization inspection completed")
    print("=" * 50)
    print(f"   Baseline status: {results['baseline_status']}")
    print(f"   Adaptive anomalies: {len(adaptive_result.get('anomalies', []))}")
    print(f"   Static issues: {len(static_proposals)}")
    print(f"   Configuration differences: {len(config_diffs)}")
    print(f"   Actions taken: {', '.join(results['actions_taken']) or 'None'}")

    if results['peer_directive']:
        print(f"\n⚠️ Peer directive generated, please check: {results['peer_directive']}")

    return results


def main():
    import argparse
    parser = argparse.ArgumentParser(description='CCCC Self-Optimization Inspection')
    parser.add_argument('--quick', action='store_true', help='Quick mode (skip baseline update)')
    parser.add_argument('--apply', action='store_true', help='Auto-apply suggestions (not yet implemented)')
    parser.add_argument('--json', action='store_true', help='Output in JSON format')
    args = parser.parse_args()

    cwd = Path.cwd()
    home = cwd / '.cccc'

    if not home.exists():
        print("Error: .cccc directory not found", file=sys.stderr)
        sys.exit(1)

    results = run_self_optimization(home, quick=args.quick, auto_apply=args.apply)

    if args.json:
        # Clean non-serializable content
        clean_results = {
            'timestamp': results['timestamp'],
            'baseline_status': results['baseline_status'],
            'anomaly_count': len(results.get('adaptive_analysis', {}).get('anomalies', [])),
            'config_differences': len(results.get('config_differences', [])),
            'actions_taken': results['actions_taken'],
            'peer_directive': results['peer_directive'],
        }
        print(json.dumps(clean_results, indent=2, ensure_ascii=False))


if __name__ == '__main__':
    main()
