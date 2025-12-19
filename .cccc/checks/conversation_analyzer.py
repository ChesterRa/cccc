#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
CCCC Conversation Analyzer

Analyze AI conversation history to extract task patterns, common issues, and successful practices.
Generate optimization suggestions for FOREMAN_TASK.md.

Core principle: Dialogue content > Config parameters. Learn from actual work, not tuning.
"""

import re
import json
from collections import defaultdict, Counter
from datetime import datetime
from pathlib import Path
from typing import Dict, List, Any, Optional, Tuple
from dataclasses import dataclass, field


@dataclass
class ParsedMessage:
    """Parsed message structure"""
    mid: str = ""
    timestamp: str = ""
    source: str = ""  # foreman / cccc / user
    items: List[Dict[str, Any]] = field(default_factory=list)
    raw_text: str = ""


@dataclass
class TaskPattern:
    """Task pattern"""
    label: str  # Task label (e.g., health, lint.preflight)
    frequency: int = 0
    outcomes: List[str] = field(default_factory=list)
    common_risks: List[str] = field(default_factory=list)
    common_next_steps: List[str] = field(default_factory=list)
    success_rate: float = 0.0


@dataclass
class ConversationInsights:
    """Conversation analysis insights"""
    total_messages: int = 0
    time_span_hours: float = 0.0
    
    # Task patterns
    task_patterns: Dict[str, TaskPattern] = field(default_factory=dict)
    
    # Issues and risks
    recurring_risks: List[Dict[str, Any]] = field(default_factory=list)
    unanswered_asks: List[Dict[str, Any]] = field(default_factory=list)
    
    # Successful practices
    effective_evidence_patterns: List[str] = field(default_factory=list)
    common_file_areas: List[str] = field(default_factory=list)
    
    # Improvement suggestions
    task_suggestions: List[Dict[str, Any]] = field(default_factory=list)
    
    # Metadata
    analyzed_at: str = ""


def load_conversation_files(mailbox_path: Path, max_files: int = 200) -> List[Tuple[Path, str]]:
    """Load conversation files"""
    files = []
    
    for peer_dir in ['peerA', 'peerB']:
        processed_dir = mailbox_path / peer_dir / 'processed'
        if not processed_dir.exists():
            continue
            
        for f in sorted(processed_dir.glob('*.txt'), reverse=True)[:max_files]:
            try:
                content = f.read_text(encoding='utf-8', errors='replace')
                files.append((f, content))
            except Exception:
                continue
    
    return files[:max_files]


def parse_message(filepath: Path, content: str) -> ParsedMessage:
    """Parse a single message"""
    msg = ParsedMessage()
    msg.raw_text = content
    
    # Extract MID
    mid_match = re.search(r'\[MID:\s*([^\]]+)\]', content)
    if mid_match:
        msg.mid = mid_match.group(1).strip()
    
    # Extract timestamp
    ts_match = re.search(r'\[TS:\s*([^\]]+)\]', content)
    if ts_match:
        msg.timestamp = ts_match.group(1).strip()
    
    # Determine source
    filename = filepath.name
    if 'foreman' in filename:
        msg.source = 'foreman'
    elif 'cccc' in filename:
        msg.source = 'cccc'
    else:
        msg.source = 'unknown'
    
    # Extract TO_PEER block content
    to_peer_match = re.search(r'<TO_PEER>(.*?)</TO_PEER>', content, re.DOTALL)
    if to_peer_match:
        peer_content = to_peer_match.group(1)
        msg.items = parse_items(peer_content)
    
    return msg


def parse_items(content: str) -> List[Dict[str, Any]]:
    """Parse Item structures in message"""
    items = []
    
    # Match Item(...): title format
    item_pattern = r'Item\(([^)]*)\):\s*(.+?)(?=\nItem\(|$)'
    item_matches = re.findall(item_pattern, content, re.DOTALL)
    
    # Filter out template placeholders
    placeholder_labels = {'<label>', 'label', '<optional second label>', '...'}
    item_matches = [(label, body) for label, body in item_matches 
                    if label.strip().lower() not in placeholder_labels and not label.startswith('<')]
    
    for label, body in item_matches:
        item = {
            'label': label.strip(),
            'body': body.strip(),
            'outcome': '',
            'why': '',
            'opposite': '',
            'progress': [],
            'evidence': [],
            'asks': [],
            'risks': [],
            'next': '',
            'files': []
        }
        
        # Extract Outcome
        outcome_match = re.search(r'Outcome:\s*(.+?)(?=;|Why:|Opposite:|Progress|Evidence|Ask|Risk|Next|Files|$)', body, re.IGNORECASE)
        if outcome_match:
            item['outcome'] = outcome_match.group(1).strip()
        
        # Extract Why
        why_match = re.search(r'Why:\s*(.+?)(?=;|Opposite:|Progress|Evidence|Ask|Risk|Next|Files|$)', body, re.IGNORECASE)
        if why_match:
            item['why'] = why_match.group(1).strip()
        
        # Extract Progress
        progress_matches = re.findall(r'Progress(?:\([^)]*\))?:\s*(.+?)(?=\n|Evidence|Ask|Risk|Next|Files|$)', body, re.IGNORECASE)
        item['progress'] = [p.strip() for p in progress_matches if p.strip()]
        
        # Extract Evidence
        evidence_matches = re.findall(r'Evidence(?:\([^)]*\))?:\s*(.+?)(?=\n|Ask|Risk|Next|Files|$)', body, re.IGNORECASE)
        item['evidence'] = [e.strip() for e in evidence_matches if e.strip()]
        
        # Extract Ask
        ask_matches = re.findall(r'Ask\(([^)]*)\):\s*(.+?)(?=\n|Counter|Risk|Next|Files|$)', body, re.IGNORECASE)
        for params, text in ask_matches:
            item['asks'].append({'params': params, 'text': text.strip()})
        
        # Extract Risk
        risk_matches = re.findall(r'Risk\(([^)]*)\):\s*(.+?)(?=\n|Next|Files|$)', body, re.IGNORECASE)
        for params, text in risk_matches:
            sev = 'med'
            sev_match = re.search(r'sev=(\w+)', params)
            if sev_match:
                sev = sev_match.group(1)
            item['risks'].append({'severity': sev, 'text': text.strip()})
        
        # Extract Next
        next_match = re.search(r'Next(?:\([^)]*\))?:\s*(.+?)(?=\n|Files|$)', body, re.IGNORECASE)
        if next_match:
            item['next'] = next_match.group(1).strip()
        
        # Extract Files
        files_match = re.search(r'Files:\s*(.+?)(?=\n|$)', body, re.IGNORECASE)
        if files_match:
            item['files'] = [f.strip() for f in files_match.group(1).split(';')]
        
        items.append(item)
    
    return items


def analyze_conversations(messages: List[ParsedMessage]) -> ConversationInsights:
    """Analyze conversations and extract insights"""
    insights = ConversationInsights()
    insights.total_messages = len(messages)
    insights.analyzed_at = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    
    # Collect task statistics
    task_stats = defaultdict(lambda: {
        'count': 0,
        'outcomes': [],
        'risks': [],
        'next_steps': [],
        'has_evidence': 0
    })
    
    # Collect all risks
    all_risks = []
    
    # Collect all Asks
    all_asks = []
    
    # Collect file areas
    file_areas = Counter()
    
    # Collect evidence patterns
    evidence_patterns = []
    
    for msg in messages:
        for item in msg.items:
            label = item['label']
            
            # Count tasks
            task_stats[label]['count'] += 1
            if item['outcome']:
                task_stats[label]['outcomes'].append(item['outcome'])
            if item['next']:
                task_stats[label]['next_steps'].append(item['next'])
            if item['evidence']:
                task_stats[label]['has_evidence'] += 1
                evidence_patterns.extend(item['evidence'])
            
            # Collect risks
            for risk in item['risks']:
                all_risks.append({
                    'label': label,
                    'severity': risk['severity'],
                    'text': risk['text'],
                    'source': msg.source
                })
                task_stats[label]['risks'].append(risk['text'])
            
            # Collect Asks
            for ask in item['asks']:
                all_asks.append({
                    'label': label,
                    'params': ask['params'],
                    'text': ask['text'],
                    'source': msg.source
                })
            
            # Count file areas
            for f in item['files']:
                # Extract directory/module
                parts = f.split('/')
                if len(parts) >= 2:
                    area = '/'.join(parts[:2])
                    file_areas[area] += 1
    
    # Build task patterns
    for label, stats in task_stats.items():
        pattern = TaskPattern(label=label)
        pattern.frequency = stats['count']
        pattern.outcomes = stats['outcomes'][:5]  # Last 5
        pattern.common_risks = _get_common_items(stats['risks'], 3)
        pattern.common_next_steps = _get_common_items(stats['next_steps'], 3)
        pattern.success_rate = stats['has_evidence'] / stats['count'] if stats['count'] > 0 else 0
        insights.task_patterns[label] = pattern
    
    # Identify recurring risks (themes appearing >=2 times)
    risk_texts = [r['text'] for r in all_risks]
    risk_themes = _extract_themes(risk_texts)
    insights.recurring_risks = [
        {'theme': theme, 'count': count, 'examples': [r for r in all_risks if theme.lower() in r['text'].lower()][:2]}
        for theme, count in risk_themes.items() if count >= 2
    ]
    
    # Identify unanswered Asks (simplified: collect all Asks)
    insights.unanswered_asks = all_asks[:10]
    
    # Common file areas
    insights.common_file_areas = [area for area, _ in file_areas.most_common(10)]
    
    # Effective evidence patterns
    insights.effective_evidence_patterns = _extract_evidence_patterns(evidence_patterns)
    
    # Generate task suggestions
    insights.task_suggestions = _generate_task_suggestions(insights)
    
    return insights


def _get_common_items(items: List[str], top_n: int = 3) -> List[str]:
    """Get most common items"""
    if not items:
        return []
    counter = Counter(items)
    return [item for item, _ in counter.most_common(top_n)]


def _extract_themes(texts: List[str]) -> Dict[str, int]:
    """Extract themes from texts"""
    themes = Counter()
    
    # Keyword list
    keywords = [
        'test', 'pnpm test', 'db:migrate', 'type-check', 'lint',
        'TODO', 'ScrapeService', 'Cookie', 'database', 'migration',
        'openspec', 'validate', 'verify', 'check', 'not run'
    ]
    
    for text in texts:
        text_lower = text.lower()
        for kw in keywords:
            if kw.lower() in text_lower:
                themes[kw] += 1
    
    return dict(themes)


def _extract_evidence_patterns(evidence_list: List[str]) -> List[str]:
    """Extract effective evidence patterns"""
    patterns = []
    
    # Common effective evidence formats
    effective_patterns = [
        r'cmd:[^:]+::OK',  # Command execution success
        r'commit:[a-f0-9]+',  # Git commit
        r'files:[^,]+#L\d+-L\d+',  # File reference
    ]
    
    for evidence in evidence_list:
        for pattern in effective_patterns:
            if re.search(pattern, evidence):
                match = re.search(pattern, evidence)
                if match:
                    patterns.append(match.group(0))
    
    return list(set(patterns))[:10]


def _generate_task_suggestions(insights: ConversationInsights) -> List[Dict[str, Any]]:
    """Generate task optimization suggestions based on insights"""
    suggestions = []
    
    # 1. High-frequency task suggestions
    high_freq_tasks = sorted(
        insights.task_patterns.items(),
        key=lambda x: x[1].frequency,
        reverse=True
    )[:5]
    
    for label, pattern in high_freq_tasks:
        if pattern.frequency >= 3:
            suggestion = {
                'type': 'high_frequency_task',
                'label': label,
                'frequency': pattern.frequency,
                'recommendation': f"Consider adding '{label}' to Standing work, frequency: {pattern.frequency}x",
                'common_risks': pattern.common_risks,
                'success_rate': f"{pattern.success_rate:.0%}"
            }
            suggestions.append(suggestion)
    
    # 2. Recurring risk suggestions
    for risk in insights.recurring_risks[:3]:
        suggestions.append({
            'type': 'recurring_risk',
            'theme': risk['theme'],
            'count': risk['count'],
            'recommendation': f"'{risk['theme']}' risk appeared {risk['count']} times, consider adding to patrol checklist or automation"
        })
    
    # 3. File area focus suggestions
    if insights.common_file_areas:
        suggestions.append({
            'type': 'focus_areas',
            'areas': insights.common_file_areas[:5],
            'recommendation': "These areas change frequently, can be Foreman's focus areas"
        })
    
    # 4. Evidence practice suggestions
    if insights.effective_evidence_patterns:
        suggestions.append({
            'type': 'evidence_patterns',
            'patterns': insights.effective_evidence_patterns[:5],
            'recommendation': "These evidence formats work well, consider promoting their use"
        })
    
    return suggestions


def generate_foreman_task_proposal(insights: ConversationInsights, current_task_path: Path) -> str:
    """Generate FOREMAN_TASK.md optimization proposal"""
    
    # Read current task file if exists
    current_content = ""
    if current_task_path.exists():
        try:
            current_content = current_task_path.read_text(encoding='utf-8')
        except Exception:
            pass
    
    lines = [
        "# FOREMAN_TASK.md Optimization Proposal",
        f"Generated: {insights.analyzed_at}",
        f"Based on {insights.total_messages} historical conversations",
        "",
        "## High-Frequency Task Patterns",
        ""
    ]
    
    # High-frequency tasks
    for label, pattern in sorted(
        insights.task_patterns.items(),
        key=lambda x: x[1].frequency,
        reverse=True
    )[:5]:
        lines.append(f"### {label}")
        lines.append(f"- Frequency: {pattern.frequency}x")
        lines.append(f"- Success rate: {pattern.success_rate:.0%}")
        if pattern.common_risks:
            lines.append(f"- Common risks: {', '.join(pattern.common_risks[:2])}")
        if pattern.common_next_steps:
            lines.append(f"- Common next steps: {pattern.common_next_steps[0][:60]}...")
        lines.append("")
    
    # Recurring risks
    if insights.recurring_risks:
        lines.extend([
            "## Recurring Risks",
            ""
        ])
        for risk in insights.recurring_risks[:5]:
            lines.append(f"- **{risk['theme']}** ({risk['count']}x)")
        lines.append("")
    
    # Low success rate tasks diagnosis (for Foreman AI analysis)
    low_success_tasks = [
        (label, pattern) for label, pattern in insights.task_patterns.items()
        if pattern.frequency >= 3 and pattern.success_rate < 0.5
    ]
    low_success_tasks.sort(key=lambda x: x[1].success_rate)
    
    if low_success_tasks:
        lines.extend([
            "## Low Success Rate Tasks Diagnosis",
            "",
            "> **Foreman Action Required**: Review each task below and generate specific improvement actions.",
            "> Consider: What commands could address the common risks? What checks should be added to patrol?",
            ""
        ])
        
        for label, pattern in low_success_tasks[:5]:
            lines.append(f"### {label}")
            lines.append(f"- **Success Rate**: {pattern.success_rate:.0%} (frequency: {pattern.frequency}x)")
            lines.append(f"- **Status**: {'Critical' if pattern.success_rate == 0 else 'Needs Improvement'}")
            lines.append("")
            
            if pattern.common_risks:
                lines.append("**Common Risks Observed**:")
                for i, risk in enumerate(pattern.common_risks[:3], 1):
                    lines.append(f"  {i}. {risk}")
                lines.append("")
            
            if pattern.common_next_steps:
                lines.append("**Historically Suggested Next Steps**:")
                for i, step in enumerate(pattern.common_next_steps[:3], 1):
                    # Truncate long steps
                    display_step = step[:100] + "..." if len(step) > 100 else step
                    lines.append(f"  {i}. {display_step}")
                lines.append("")
            
            lines.append("---")
            lines.append("")
        
        # Add instruction block for Foreman
        lines.extend([
            "### Foreman Analysis Instructions",
            "",
            "Based on the above diagnosis, please:",
            "1. Identify root causes for each low success rate task",
            "2. Generate specific, executable commands to address the risks",
            "3. Propose additions to the patrol checklist",
            "4. Consider if any automation could prevent these issues",
            ""
        ])

    
    # Focus areas
    if insights.common_file_areas:
        lines.extend([
            "## Change Hotspot Areas",
            ""
        ])
        for area in insights.common_file_areas[:5]:
            lines.append(f"- `{area}`")
        lines.append("")
    
    # Suggested Standing work
    lines.extend([
        "## Suggested Standing Work Updates",
        "",
        "Based on conversation analysis, suggest updating FOREMAN_TASK.md Standing work:",
        "",
        "```markdown",
        "Standing work (auto-generated from history)",
    ])
    
    # Generate standing work from high-frequency tasks
    for label, pattern in sorted(
        insights.task_patterns.items(),
        key=lambda x: x[1].frequency,
        reverse=True
    )[:3]:
        if pattern.frequency >= 2:
            lines.append(f"- {label}: auto-patrol (freq {pattern.frequency}x, success {pattern.success_rate:.0%})")
    
    # Generate standing work from recurring risks
    for risk in insights.recurring_risks[:2]:
        lines.append(f"- Check {risk['theme']} (risk appeared {risk['count']}x)")
    
    lines.extend([
        "```",
        "",
        "## Suggested Focus Areas",
        "",
        "```markdown",
        "Useful references (based on change hotspots)",
    ])
    
    for area in insights.common_file_areas[:5]:
        lines.append(f"- {area}")
    
    lines.extend([
        "```",
        ""
    ])
    
    # Structured suggestions in JSON format (for programmatic use)
    lines.extend([
        "## Structured Suggestions (JSON)",
        "",
        "```json"
    ])
    
    structured = {
        'analyzed_at': insights.analyzed_at,
        'total_messages': insights.total_messages,
        'suggestions': insights.task_suggestions,
        'high_frequency_tasks': [
            {'label': label, 'frequency': p.frequency, 'success_rate': p.success_rate}
            for label, p in sorted(insights.task_patterns.items(), key=lambda x: -x[1].frequency)[:5]
        ],
        'low_success_tasks': [
            {
                'label': label,
                'success_rate': p.success_rate,
                'frequency': p.frequency,
                'status': 'critical' if p.success_rate == 0 else 'needs_improvement',
                'common_risks': p.common_risks[:3],
                'suggested_next_steps': p.common_next_steps[:3]
            }
            for label, p in sorted(insights.task_patterns.items(), key=lambda x: x[1].success_rate)
            if p.frequency >= 3 and p.success_rate < 0.5
        ][:5],
        'recurring_risks': [{'theme': r['theme'], 'count': r['count']} for r in insights.recurring_risks[:5]],
        'focus_areas': insights.common_file_areas[:5]
    }
    
    lines.append(json.dumps(structured, indent=2, ensure_ascii=False))
    lines.extend([
        "```",
        ""
    ])
    
    return "\n".join(lines)


def update_foreman_task(task_path: Path, insights: ConversationInsights, home: Path = None, backup: bool = True) -> Dict[str, Any]:
    """Update FOREMAN_TASK.md with optimization suggestions
    
    Safely merges new suggestions into existing FOREMAN_TASK.md:
    - Creates backup in .cccc/work/foreman/ before modification
    - Adds/updates 'Standing work (auto-generated)' section
    - Adds/updates 'Focus areas (auto-generated)' section
    - Preserves all other content
    """
    result = {'status': 'skipped', 'changes': []}
    
    # Read existing content
    existing_content = ""
    if task_path.exists():
        try:
            existing_content = task_path.read_text(encoding='utf-8')
        except Exception as e:
            return {'status': 'error', 'error': f'Failed to read: {e}'}
    
    # Create backup in .cccc/work/foreman/
    if backup and existing_content and home:
        backup_dir = home / 'work' / 'foreman'
        backup_dir.mkdir(parents=True, exist_ok=True)
        timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
        backup_path = backup_dir / f'FOREMAN_TASK.{timestamp}.bak'
        try:
            backup_path.write_text(existing_content, encoding='utf-8')
            result['backup_path'] = str(backup_path)
        except Exception:
            pass
    
    # Generate new sections
    standing_work_lines = []
    focus_area_lines = []
    
    # High-frequency tasks -> Standing work
    for label, pattern in sorted(
        insights.task_patterns.items(),
        key=lambda x: x[1].frequency,
        reverse=True
    )[:5]:
        if pattern.frequency >= 3:
            standing_work_lines.append(
                f"- {label}: patrol (freq {pattern.frequency}x, success {pattern.success_rate:.0%})"
            )
    
    # Recurring risks -> Standing work
    for risk in insights.recurring_risks[:3]:
        if risk['count'] >= 3:
            standing_work_lines.append(
                f"- Check {risk['theme']} (risk appeared {risk['count']}x)"
            )
    
    # Focus areas
    for area in insights.common_file_areas[:5]:
        if area and area != 'n/a':
            focus_area_lines.append(f"- {area}")
    
    # Low success rate tasks diagnosis
    low_success_lines = []
    low_success_tasks = [
        (label, pattern) for label, pattern in insights.task_patterns.items()
        if pattern.frequency >= 3 and pattern.success_rate < 0.5
    ]
    low_success_tasks.sort(key=lambda x: x[1].success_rate)
    
    for label, pattern in low_success_tasks[:5]:
        status = "🔴 Critical" if pattern.success_rate == 0 else "🟡 Needs Improvement"
        low_success_lines.append(f"### {label} ({pattern.success_rate:.0%})")
        low_success_lines.append(f"- Status: {status}")
        low_success_lines.append(f"- Frequency: {pattern.frequency}x")
        if pattern.common_risks:
            low_success_lines.append(f"- Risk: {pattern.common_risks[0][:80]}...")
        if pattern.common_next_steps:
            low_success_lines.append(f"- Suggested: {pattern.common_next_steps[0][:80]}...")
        low_success_lines.append("")
    
    if not standing_work_lines and not focus_area_lines and not low_success_lines:
        result['status'] = 'no_changes'
        result['reason'] = 'No significant patterns found'
        return result
    
    # Build new sections
    new_sections = []
    
    if standing_work_lines:
        new_sections.append("\n## Standing work (auto-generated)\n")
        new_sections.append("\n".join(standing_work_lines))
        result['changes'].append(f'Added {len(standing_work_lines)} standing work items')
    
    if focus_area_lines:
        new_sections.append("\n\n## Focus areas (auto-generated)\n")
        new_sections.append("\n".join(focus_area_lines))
        result['changes'].append(f'Added {len(focus_area_lines)} focus areas')
    
    if low_success_lines:
        new_sections.append("\n\n## Low success tasks (auto-generated)\n")
        new_sections.append("> Foreman: Review these tasks and generate improvement actions.\n\n")
        new_sections.append("\n".join(low_success_lines))
        result['changes'].append(f'Added {len(low_success_tasks[:5])} low success task diagnosis')
    
    new_section_text = "".join(new_sections)
    
    # Remove old auto-generated sections if they exist
    import re
    cleaned_content = existing_content
    cleaned_content = re.sub(
        r'\n## Standing work \(auto-generated\)\n[\s\S]*?(?=\n## |$)',
        '',
        cleaned_content
    )
    cleaned_content = re.sub(
        r'\n## Focus areas \(auto-generated\)\n[\s\S]*?(?=\n## |$)',
        '',
        cleaned_content
    )
    cleaned_content = re.sub(
        r'\n## Low success tasks \(auto-generated\)\n[\s\S]*?(?=\n## |$)',
        '',
        cleaned_content
    )
    
    # Append new sections
    final_content = cleaned_content.rstrip() + "\n" + new_section_text + "\n"
    
    # Write updated content
    try:
        task_path.parent.mkdir(parents=True, exist_ok=True)
        task_path.write_text(final_content, encoding='utf-8')
        result['status'] = 'success'
        result['path'] = str(task_path)
    except Exception as e:
        result['status'] = 'error'
        result['error'] = str(e)
    
    return result


def run_conversation_analysis(home: Path, auto_update: bool = False) -> Dict[str, Any]:
    """Execute complete conversation analysis workflow
    
    Args:
        home: .cccc directory path
        auto_update: If True, automatically update FOREMAN_TASK.md with suggestions
    """
    mailbox_path = home / 'mailbox'
    output_dir = home / 'work' / 'foreman' / 'diagnosis'
    
    # 1. Load conversation files
    print("📂 Loading conversation history...")
    files = load_conversation_files(mailbox_path)
    
    if len(files) < 5:
        print(f"⚠️ Insufficient data ({len(files)} messages), skipping analysis")
        return {'status': 'insufficient_data', 'message_count': len(files)}
    
    # 2. Parse messages
    print(f"📝 Parsing {len(files)} messages...")
    messages = []
    for filepath, content in files:
        msg = parse_message(filepath, content)
        if msg.items:  # Only keep messages with structured content
            messages.append(msg)
    
    print(f"   Valid messages: {len(messages)}")
    
    # 3. Analyze conversations
    print("🔍 Analyzing conversation patterns...")
    insights = analyze_conversations(messages)
    
    # 4. Generate proposal
    print("📋 Generating optimization proposal...")
    current_task_path = home.parent / 'FOREMAN_TASK.md'
    proposal = generate_foreman_task_proposal(insights, current_task_path)
    
    # 5. Save reports
    output_dir.mkdir(parents=True, exist_ok=True)
    
    proposal_path = output_dir / 'task_optimization_proposal.md'
    proposal_path.write_text(proposal, encoding='utf-8')
    
    # Save structured insights
    insights_path = output_dir / 'conversation_insights.json'
    
    # Build low success tasks list for JSON output
    low_success_tasks = [
        {
            'label': label,
            'success_rate': p.success_rate,
            'frequency': p.frequency,
            'status': 'critical' if p.success_rate == 0 else 'needs_improvement',
            'common_risks': p.common_risks[:3],
            'suggested_next_steps': p.common_next_steps[:3]
        }
        for label, p in sorted(insights.task_patterns.items(), key=lambda x: x[1].success_rate)
        if p.frequency >= 3 and p.success_rate < 0.5
    ][:5]
    
    insights_data = {
        'analyzed_at': insights.analyzed_at,
        'total_messages': insights.total_messages,
        'task_patterns': {
            label: {
                'frequency': p.frequency,
                'success_rate': p.success_rate,
                'common_risks': p.common_risks,
                'common_next_steps': p.common_next_steps
            }
            for label, p in insights.task_patterns.items()
        },
        'low_success_tasks': low_success_tasks,
        'recurring_risks': insights.recurring_risks,
        'common_file_areas': insights.common_file_areas,
        'suggestions': insights.task_suggestions
    }
    with open(insights_path, 'w', encoding='utf-8') as f:
        json.dump(insights_data, f, indent=2, ensure_ascii=False)
    
    print(f"\n✅ Analysis complete")
    print(f"   Task patterns: {len(insights.task_patterns)}")
    print(f"   Recurring risks: {len(insights.recurring_risks)}")
    print(f"   Suggestions: {len(insights.task_suggestions)}")
    print(f"\n📄 Reports saved:")
    print(f"   - {proposal_path}")
    print(f"   - {insights_path}")
    
    result = {
        'status': 'success',
        'message_count': len(messages),
        'task_pattern_count': len(insights.task_patterns),
        'suggestion_count': len(insights.task_suggestions),
        'proposal_path': str(proposal_path),
        'insights_path': str(insights_path),
        'suggestions': insights.task_suggestions
    }
    
    # Auto-update FOREMAN_TASK.md if requested
    if auto_update:
        print(f"\n📝 Updating FOREMAN_TASK.md...")
        update_result = update_foreman_task(current_task_path, insights, home)
        result['foreman_task_update'] = update_result
        
        if update_result['status'] == 'success':
            print(f"   ✅ Updated: {update_result['path']}")
            for change in update_result.get('changes', []):
                print(f"      - {change}")
        elif update_result['status'] == 'no_changes':
            print(f"   ⏭️ Skipped: {update_result.get('reason', 'No changes needed')}")
        else:
            print(f"   ❌ Failed: {update_result.get('error', 'Unknown error')}")
    
    return result


def main():
    """Command-line entry point"""
    import sys
    import argparse
    
    parser = argparse.ArgumentParser(description='CCCC Conversation Analyzer')
    parser.add_argument('--json', action='store_true', help='Output in JSON format')
    parser.add_argument('--update', action='store_true', 
                        help='Auto-update FOREMAN_TASK.md with suggestions')
    args = parser.parse_args()
    
    cwd = Path.cwd()
    home = cwd / '.cccc'
    
    if not home.exists():
        print("Error: .cccc directory not found", file=sys.stderr)
        sys.exit(1)
    
    result = run_conversation_analysis(home, auto_update=args.update)
    
    if args.json:
        print(json.dumps(result, indent=2, ensure_ascii=False))


if __name__ == '__main__':
    main()
