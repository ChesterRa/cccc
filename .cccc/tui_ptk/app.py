#!/usr/bin/env python3
"""
CCCC PTK TUI - Modern Interactive Orchestrator Interface

Production-ready CLI interface for dual-agent collaboration with CCCC.
Meets 2025 CLI standards for usability, aesthetics, and functionality.

Core Features:
  • Setup: Elegant actor configuration with visual hierarchy
  • Runtime: Real-time collaborative CLI with Timeline, Input, Status
  • Commands: /a, /b, /both, /help, /pause, /resume, /restart, /quit, /foreman, /aux, /verbose on|off

UI/UX Excellence:
  • Modern 256-color scheme with semantic colors (success/warning/error/info)
  • Dynamic prompt showing current mode (normal/search) and connection state
  • Smart message coloring by type (PeerA/PeerB/System/User)
  • Enhanced status panel with connection indicator, message count, timestamps
  • Visual feedback for all operations (✓ success, ⚠ warning, error messages)

Input & Editing:
  • Command auto-completion with Tab
  • Command history (1000 commands, Up/Down navigation)
  • Ctrl+R reverse search with live preview
  • Standard editing shortcuts (Ctrl+A/E/W/U/K)
  • Input validation with helpful error messages

Navigation:
  • PageUp/PageDown: Scroll timeline
  • Shift+G: Jump to bottom (latest messages)
  • gg: Jump to top (oldest messages)
  • Ctrl+L: Clear screen
  • Mouse drag: Select and auto-copy to clipboard

Connection & Status:
  • Real-time connection monitoring (● connected / ○ disconnected)
  • Live message count and handoff statistics
  • Last update time display
  • Automatic reconnection handling

Design Philosophy:
  • Unified visual language (❯ prompts, ● indicators, consistent symbols)
  • Flexible responsive layout (adapts to window size, min 10 lines)
  • High information density without clutter
  • Semantic color usage for instant recognition
  • Cohesive and elegant overall aesthetic
"""
from __future__ import annotations

import asyncio
import json
import os
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from prompt_toolkit import Application
from prompt_toolkit.application import get_app
from prompt_toolkit.completion import Completer, Completion, ThreadedCompleter
from prompt_toolkit.document import Document
from prompt_toolkit.filters import Condition, has_focus
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.layout import (
    Layout, HSplit, VSplit, Window, Float, FloatContainer,
    FormattedTextControl, Dimension, ScrollablePane, ConditionalContainer
)
from prompt_toolkit.widgets import (
    TextArea, Button, Dialog, RadioList, Label, Frame
)
from prompt_toolkit.styles import Style
from prompt_toolkit.mouse_events import MouseEventType
from prompt_toolkit.shortcuts import radiolist_dialog
from asyncio import create_task


def _is_wsl() -> bool:
    """Detect if running in WSL (Windows Subsystem for Linux)."""
    # Check WSL-specific environment variables
    if os.environ.get('WSL_DISTRO_NAME') or os.environ.get('WSL_INTEROP') or os.environ.get('WSLENV'):
        return True
    # Check /proc/version for WSL signatures
    try:
        with open('/proc/version', 'r') as f:
            version = f.read().lower()
            if 'microsoft' in version or 'wsl' in version:
                return True
    except Exception:
        pass
    return False


def get_clipboard_image(save_dir: Path) -> Optional[str]:
    """Check clipboard for image and save to file if found.
    Supports macOS, Linux (X11/Wayland), Windows, and WSL2.
    Returns the saved file path if an image was found, None otherwise.
    """
    import sys
    
    if sys.platform == 'darwin':
        return _get_clipboard_image_macos(save_dir)
    elif sys.platform == 'linux':
        # WSL2 requires special handling via PowerShell
        if _is_wsl():
            return _get_clipboard_image_wsl(save_dir)
        return _get_clipboard_image_linux(save_dir)
    elif sys.platform == 'win32':
        return _get_clipboard_image_windows(save_dir)
    return None


def _get_clipboard_image_wsl(save_dir: Path) -> Optional[str]:
    """WSL2 clipboard image extraction using PowerShell.exe via WSL interop.
    
    Strategy: Save to Windows TEMP first, then copy to WSL.
    This is more reliable than trying to save directly to WSL paths.
    """
    try:
        save_dir.mkdir(parents=True, exist_ok=True)
        timestamp = int(time.time() * 1000)
        filename = f"clipboard_{timestamp}.png"
        filepath = save_dir / filename
        
        # Get Windows temp directory
        try:
            result = subprocess.run(
                ['cmd.exe', '/c', 'echo %TEMP%'],
                capture_output=True, timeout=5
            )
            win_temp = result.stdout.decode('utf-8', errors='replace').strip()
        except Exception:
            return None
        
        if not win_temp:
            return None
        
        win_filename = f"cccc_clip_{timestamp}.png"
        win_fullpath = f"{win_temp}\\{win_filename}"
        
        # PowerShell script to save clipboard image
        ps_script = f'''
Add-Type -AssemblyName System.Windows.Forms
$img = [System.Windows.Forms.Clipboard]::GetImage()
if ($img -ne $null) {{
    $img.Save("{win_fullpath}")
    Write-Output "OK"
}} else {{
    Write-Output "NONE"
}}
'''
        result = subprocess.run(
            ['powershell.exe', '-NoProfile', '-Command', ps_script],
            capture_output=True, timeout=20
        )
        stdout = result.stdout.decode('utf-8', errors='replace').strip()
        
        if stdout != 'OK':
            return None
        
        # Convert Windows path to WSL path and copy
        try:
            result = subprocess.run(
                ['wslpath', '-u', win_fullpath],
                capture_output=True, text=True, timeout=5
            )
            wsl_temp_path = result.stdout.strip()
        except Exception:
            return None
        
        if not wsl_temp_path or not Path(wsl_temp_path).exists():
            return None
        
        # Copy to target directory
        import shutil
        shutil.copy2(wsl_temp_path, filepath)
        
        # Clean up Windows temp file
        try:
            Path(wsl_temp_path).unlink()
        except Exception:
            pass
        
        if filepath.exists():
            return str(filepath)
        
        return None
    except Exception:
        return None


def _get_clipboard_image_macos(save_dir: Path) -> Optional[str]:
    """macOS clipboard image extraction using osascript."""
    try:
        # Check if clipboard contains image data using osascript
        check_script = '''
        tell application "System Events"
            try
                set clipboardInfo to (clipboard info)
                repeat with i in clipboardInfo
                    if (first item of i) is «class PNGf» then
                        return "PNG"
                    else if (first item of i) is «class TIFF» then
                        return "TIFF"
                    end if
                end repeat
            end try
        end tell
        return "NONE"
        '''
        result = subprocess.run(
            ['osascript', '-e', check_script],
            capture_output=True, text=True, timeout=5
        )
        clip_type = result.stdout.strip()

        if clip_type not in ('PNG', 'TIFF'):
            return None

        # Create save directory if needed
        save_dir.mkdir(parents=True, exist_ok=True)

        # Generate unique filename
        timestamp = int(time.time() * 1000)
        filename = f"clipboard_{timestamp}.png"
        filepath = save_dir / filename

        # Save clipboard image using osascript
        save_script = f'''
        set saveFile to POSIX file "{filepath}"
        try
            set imageData to the clipboard as «class PNGf»
            set fileRef to open for access saveFile with write permission
            write imageData to fileRef
            close access fileRef
            return "OK"
        on error
            try
                close access saveFile
            end try
            return "ERROR"
        end try
        '''
        result = subprocess.run(
            ['osascript', '-e', save_script],
            capture_output=True, text=True, timeout=10
        )

        if result.stdout.strip() == 'OK' and filepath.exists():
            return str(filepath)
        return None

    except Exception:
        return None


def _get_clipboard_image_linux(save_dir: Path) -> Optional[str]:
    """Linux clipboard image extraction using xclip or wl-paste."""
    try:
        save_dir.mkdir(parents=True, exist_ok=True)
        timestamp = int(time.time() * 1000)
        filename = f"clipboard_{timestamp}.png"
        filepath = save_dir / filename
        
        # Try wl-paste first (Wayland)
        try:
            result = subprocess.run(
                ['wl-paste', '--type', 'image/png'],
                capture_output=True, timeout=5
            )
            if result.returncode == 0 and result.stdout:
                filepath.write_bytes(result.stdout)
                return str(filepath)
        except (FileNotFoundError, subprocess.TimeoutExpired):
            pass
        
        # Try xclip (X11)
        try:
            result = subprocess.run(
                ['xclip', '-selection', 'clipboard', '-t', 'image/png', '-o'],
                capture_output=True, timeout=5
            )
            if result.returncode == 0 and result.stdout:
                filepath.write_bytes(result.stdout)
                return str(filepath)
        except (FileNotFoundError, subprocess.TimeoutExpired):
            pass
        
        return None
    except Exception:
        return None


def _get_clipboard_image_windows(save_dir: Path) -> Optional[str]:
    """Windows clipboard image extraction using PowerShell."""
    try:
        save_dir.mkdir(parents=True, exist_ok=True)
        timestamp = int(time.time() * 1000)
        filename = f"clipboard_{timestamp}.png"
        filepath = save_dir / filename
        
        # PowerShell script to save clipboard image
        ps_script = f'''
        Add-Type -AssemblyName System.Windows.Forms
        $img = [System.Windows.Forms.Clipboard]::GetImage()
        if ($img -ne $null) {{
            $img.Save("{filepath}")
            Write-Output "OK"
        }} else {{
            Write-Output "NONE"
        }}
        '''
        result = subprocess.run(
            ['powershell', '-NoProfile', '-Command', ps_script],
            capture_output=True, text=True, timeout=10
        )
        
        if result.stdout.strip() == 'OK' and filepath.exists():
            return str(filepath)
        return None
    except Exception:
        return None


def get_clipboard_text() -> Optional[str]:
    """Get text from clipboard (cross-platform, including WSL2)."""
    import sys
    try:
        if sys.platform == 'darwin':
            result = subprocess.run(['pbpaste'], capture_output=True, text=True, timeout=2)
            return result.stdout if result.returncode == 0 else None
        elif sys.platform == 'linux':
            # WSL2: use PowerShell.exe to access Windows clipboard
            if _is_wsl():
                try:
                    result = subprocess.run(
                        ['powershell.exe', '-NoProfile', '-Command', 'Get-Clipboard'],
                        capture_output=True, text=True, timeout=5
                    )
                    if result.returncode == 0 and result.stdout:
                        return result.stdout
                except Exception:
                    pass
            # Try wl-paste first (Wayland), then xclip (X11)
            try:
                result = subprocess.run(['wl-paste'], capture_output=True, text=True, timeout=2)
                if result.returncode == 0:
                    return result.stdout
            except FileNotFoundError:
                pass
            try:
                result = subprocess.run(['xclip', '-selection', 'clipboard', '-o'], capture_output=True, text=True, timeout=2)
                if result.returncode == 0:
                    return result.stdout
            except FileNotFoundError:
                pass
            return None
        elif sys.platform == 'win32':
            result = subprocess.run(['powershell', '-NoProfile', '-Command', 'Get-Clipboard'], capture_output=True, text=True, timeout=2)
            return result.stdout if result.returncode == 0 else None
    except Exception:
        return None
    return None


def parse_message_images(msg: str) -> Tuple[str, List[str]]:
    """Parse message for image paths prefixed with @.
    Returns (cleaned_message, list_of_image_paths).
    Example: "hello @/path/to/img.png world" -> ("hello  world", ["/path/to/img.png"])
    """
    import re
    images = []
    # Match @/path/to/file.ext (common image extensions)
    pattern = r'@(/[^\s]+\.(?:png|jpg|jpeg|gif|webp|bmp|tiff))'

    def replace_match(m):
        path = m.group(1)
        if Path(path).exists():
            images.append(path)
            return ''  # Remove from message
        return m.group(0)  # Keep original if file doesn't exist

    cleaned = re.sub(pattern, replace_match, msg, flags=re.IGNORECASE)
    cleaned = ' '.join(cleaned.split())  # Normalize whitespace
    return cleaned, images


def save_image_for_cli(home: Path, image_path: str) -> Optional[Dict[str, Any]]:
    """
    Save image to unified inbound-files directory with metadata.
    Returns metadata dict with path, sha256, size, mime on success.
    
    This mirrors the Telegram/Discord/Slack bridge file handling for consistency.
    """
    import hashlib
    import mimetypes
    import shutil
    
    try:
        src = Path(image_path)
        if not src.exists():
            return None
        
        # Unified destination directory (same as IM bridges)
        dest_dir = home / "work" / "inbound-files" / "photos"
        dest_dir.mkdir(parents=True, exist_ok=True)
        
        # Generate unique filename with timestamp
        timestamp = int(time.time() * 1000)
        suffix = src.suffix or '.png'
        filename = f"tui_{timestamp}{suffix}"
        dest_path = dest_dir / filename
        
        # Copy file
        shutil.copy2(src, dest_path)
        
        # Calculate SHA256
        sha256 = hashlib.sha256(dest_path.read_bytes()).hexdigest()
        
        # Get file size and MIME type
        file_size = dest_path.stat().st_size
        mime_type = mimetypes.guess_type(str(dest_path))[0] or 'image/png'
        
        # Create sidecar metadata file (same format as Telegram bridge)
        meta = {
            'path': str(dest_path.relative_to(home.parent)),  # Relative to project root
            'sha256': sha256,
            'bytes': file_size,
            'mime': mime_type,
            'source': 'tui-clipboard',
            'ts': time.strftime('%Y-%m-%d %H:%M:%S')
        }
        
        sidecar = dest_path.with_suffix(dest_path.suffix + '.meta.json')
        sidecar.write_text(json.dumps(meta, ensure_ascii=False, indent=2), encoding='utf-8')
        
        # Append to inbound-index.jsonl (same as IM bridges)
        try:
            idx = home / "state" / "inbound-index.jsonl"
            idx.parent.mkdir(parents=True, exist_ok=True)
            rec = {
                'ts': int(time.time()),
                'path': str(dest_path),
                'platform': 'tui',
                'routes': [],  # Will be determined when sending
                'mime': mime_type,
                'bytes': file_size,
                'sha256': sha256
            }
            with idx.open('a', encoding='utf-8') as f:
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        except Exception:
            pass
        
        return meta
    except Exception:
        return None


def format_file_for_message(meta: Dict[str, Any]) -> str:
    """Format file metadata for inclusion in CLI message (same as IM bridges)."""
    return f"File: {meta['path']}\nSHA256: {meta['sha256']}  Size: {meta['bytes']}  MIME: {meta['mime']}"

# Blueprint Task Panel (lazy import to avoid circular deps)
# Use relative import since package name varies (cccc_tui_ptk_pkg vs tui_ptk)
try:
    from .task_panel import TaskPanel
except ImportError:
    TaskPanel = None  # type: ignore


class CommandCompleter(Completer):
    """Auto-completion for CCCC commands"""

    def __init__(self):
        super().__init__()
        # Define available commands with descriptions (console removed; TUI/IM are primary)
        self.commands = [
            # Routing
            ('/a', 'Send to PeerA'),
            ('/b', 'Send to PeerB'),
            ('/both', 'Send to both peers'),
            # Basic control
            ('/help', 'Show help'),
            ('/pause', 'Pause handoff'),
            ('/resume', 'Resume handoff'),
            ('/restart', 'Restart peer CLI (peera|peerb|both)'),
            ('/quit', 'Quit CCCC'),
            # Operations
            ('/foreman', 'Foreman control (on|off|status|now)'),
            ('/aux', 'Run Aux helper'),
            ('/verbose', 'Verbose on|off'),
            # Tasks
            ('/task', 'Show task status'),
        ]

    def get_completions(self, document: Document, complete_event):
        """Generate completions for commands starting with /"""
        # Get text before cursor
        text = document.text_before_cursor

        # Only show completions when text starts with /
        if not text.startswith('/'):
            return

        # Don't complete if there's already a space (command already entered)
        if ' ' in text:
            return

        # Match and yield completions
        for cmd, desc in self.commands:
            if cmd.startswith(text.lower()) or cmd.startswith(text):
                yield Completion(
                    cmd,
                    start_position=-len(text),
                    display=f'{cmd:<12} {desc}'
                )


class ClickableRadioList(RadioList):
    """RadioList that supports mouse click and Enter key to confirm selection"""

    def __init__(self, values, on_confirm=None):
        """
        Args:
            values: List of (value, label) tuples
            on_confirm: Callback function called when user clicks or presses Enter
        """
        super().__init__(values)
        self.on_confirm = on_confirm

        # Wrap mouse handler for click confirmation
        if on_confirm:
            original_handler = self.control.mouse_handler

            def new_mouse_handler(mouse_event):
                # Call original handler first (updates selection)
                result = original_handler(mouse_event) if original_handler else None

                # On mouse UP (release), confirm selection
                if mouse_event.event_type == MouseEventType.MOUSE_UP:
                    self.on_confirm()

                return result

            self.control.mouse_handler = new_mouse_handler




@dataclass
class SetupConfig:
    """Configuration state"""
    peerA: str = ''
    peerB: str = ''
    aux: str = 'none'
    foreman: str = 'none'
    self_opt_enabled: bool = False  # Self-optimization detection toggle
    mode: str = 'tmux'
    tg_token: str = ''
    tg_chat: str = ''
    # Slack: split into bot_token and app_token for clarity
    sl_bot_token: str = ''  # Bot token (xoxb-) - required for outbound
    sl_app_token: str = ''  # App token (xapp-) - required for inbound communication
    sl_chan: str = ''
    dc_token: str = ''
    dc_chan: str = ''
    # WeCom: outbound-only via webhook
    wc_webhook: str = ''  # Webhook URL for WeCom robot

    # Backward compatibility property
    @property
    def sl_token(self) -> str:
        """Backward compatibility - return bot token"""
        return self.sl_bot_token

    @sl_token.setter
    def sl_token(self, value: str):
        """Backward compatibility - set bot token"""
        self.sl_bot_token = value

    def is_valid(self, actors: List[str], home: Path) -> tuple[bool, str]:
        """Validate configuration"""
        if not self.peerA:
            return False, "PeerA actor required"
        # PeerB can be 'none' for single-peer mode
        if not self.peerB:
            return False, "PeerB actor required (use 'none' for single-peer mode)"

        # Check CLI availability
        missing = []
        for role, actor in [('PeerA', self.peerA), ('PeerB', self.peerB)]:
            if actor and actor != 'none':
                cmd = _get_actor_command(home, actor)
                if cmd and not shutil.which(cmd.split()[0]):
                    missing.append(f"{role}→{actor}")

        if missing:
            return False, f"CLI not on PATH: {', '.join(missing)}"

        if self.mode == 'telegram' and not self.tg_token:
            return False, "Telegram token required"
        if self.mode == 'slack':
            if not self.sl_bot_token:
                return False, "Slack bot token (xoxb-) required"
            if not self.sl_app_token:
                return False, "Slack app token (xapp-) required for inbound communication"
        if self.mode == 'discord' and not self.dc_token:
            return False, "Discord token required"
        if self.mode == 'wecom' and not self.wc_webhook:
            return False, "WeCom webhook URL required"

        return True, ""


def _get_actor_command(home: Path, actor: str) -> Optional[str]:
    """Get CLI command for actor"""
    try:
        import yaml
        p = home / "settings" / "agents.yaml"
        if p.exists():
            data = yaml.safe_load(p.read_text(encoding='utf-8')) or {}
            acts = data.get('actors') or {}
            peer = (acts.get(actor) or {}).get('peer') or {}
            return str(peer.get('command') or '')
    except Exception:
        pass
    return None


def check_actor_available(actor_name: str, home: Path) -> Tuple[bool, str]:
    """
    Check if an actor's CLI is available in the environment.

    Strategy:
    1. Try to load custom command from agents.yaml configuration
    2. Check if configured command exists (supports custom paths)
    3. Fallback to checking default command name in PATH
    4. Provide helpful installation hints for common actors

    Returns:
        (is_available, hint_message)
        - True, "Installed" if CLI is found
        - False, "Installation hint" if not found
    """
    # Special case: 'none' is always available
    if actor_name == 'none':
        return True, "Disabled"

    # Try to load custom command from agents.yaml
    command_from_config = None
    try:
        import yaml
        agents_file = home / "settings" / "agents.yaml"
        if agents_file.exists():
            data = yaml.safe_load(agents_file.read_text(encoding='utf-8')) or {}
            actors = data.get('actors', {})

            if actor_name in actors:
                config = actors[actor_name]
                if isinstance(config, dict):
                    # Get peer command configuration
                    peer_config = config.get('peer', {})
                    command = peer_config.get('command', '')

                    if command:
                        command_from_config = command
                        # Expand environment variables (e.g., $CLAUDE_I_CMD)
                        command = os.path.expandvars(command)
                        # Extract first token (command name/path)
                        cmd_name = command.split()[0]

                        # Check if it's an absolute path
                        if os.path.isabs(cmd_name):
                            if os.path.isfile(cmd_name) and os.access(cmd_name, os.X_OK):
                                return True, "Installed (custom)"
                        else:
                            # Check in PATH
                            if shutil.which(cmd_name):
                                return True, "Installed"
                        # Command configured but not found - don't fallback
                        # Return false immediately with installation hint
                        hint = install_hints.get(actor_name, f'{cmd_name} not found in PATH')
                        return False, hint
    except Exception:
        pass  # Silently fail and try fallback

    # Fallback: ONLY if no command was configured in agents.yaml
    # This prevents false positives (e.g., 'cursor' editor vs 'cursor-agent' CLI)
    if command_from_config is None:
        if shutil.which(actor_name):
            return True, "Installed"

    # Not found - provide accurate installation hints
    install_hints = {
        'claude': 'npm install -g @anthropic-ai/claude-code',
        'codex': 'npm i -g @openai/codex',
        'gemini': 'npm install -g @google/gemini-cli',
        'droid': 'curl -fsSL https://app.factory.ai/cli | sh',
        'opencode': 'npm i -g opencode-ai',
        'kilocode': 'npm install -g @kilocode/cli',
        'copilot': 'npm install -g @github/copilot',
        'auggie': 'npm install -g @augmentcode/auggie',
        'cursor': 'curl https://cursor.com/install -fsS | bash',
    }

    hint = install_hints.get(actor_name, 'Not found in PATH')
    return False, hint


def _write_yaml(home: Path, rel_path: str, data: Dict[str, Any]) -> None:
    """Write YAML file"""
    try:
        import yaml
        p = home / rel_path
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(yaml.safe_dump(data, allow_unicode=True, sort_keys=False), encoding='utf-8')
    except Exception:
        p = home / rel_path
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding='utf-8')


def _load_yaml(home: Path, rel_path: str) -> Dict[str, Any]:
    """Load YAML file, return empty dict if not found or on error"""
    try:
        import yaml
        p = home / rel_path
        if p.exists():
            content = yaml.safe_load(p.read_text(encoding='utf-8'))
            return content if isinstance(content, dict) else {}
        return {}
    except Exception:
        return {}


def create_header() -> Window:
    """Professional 2025 CLI header with ASCII art and integrated branding"""
    # Get CCCC version from package metadata
    try:
        from importlib.metadata import version as get_version
        version = get_version('cccc-pair')
    except Exception:
        version = 'unknown'

    # Premium header with ASCII logo and right-aligned branding
    text = [
        ('class:title', '   ╔══════════════════════════════════════════════════════════════════╗\n'),
        ('class:title', '   ║                                                                  ║\n'),
        ('class:title', '   ║      ██████╗  ██████╗  ██████╗  ██████╗                          ║\n'),
        ('class:title', '   ║     ██╔════╝ ██╔════╝ ██╔════╝ ██╔════╝                          ║\n'),
        ('class:title', '   ║     ██║      ██║      ██║      ██║         '), ('class:success.bold', 'Pair Orchestrator'), ('class:title', '     ║\n'),
        ('class:title', '   ║     ██║      ██║      ██║      ██║         '), ('class:hint', 'CLI × CLI Co-Creation'), ('class:title', ' ║\n'),
        ('class:title', '   ║     ╚██████╗ ╚██████╗ ╚██████╗ ╚██████╗                          ║\n'),
        ('class:title', '   ║      ╚═════╝  ╚═════╝  ╚═════╝  ╚═════╝                          ║\n'),
        ('class:title', '   ║                                                   '), ('class:value', f'v{version}'), ('class:title', '        ║\n'),
        ('class:title', '   ╚══════════════════════════════════════════════════════════════════╝'),
    ]
    return Window(
        content=FormattedTextControl(text),
        height=Dimension(preferred=10),
        dont_extend_height=True
    )


def create_runtime_header(status_dot: str = '●', status_color: str = 'class:status.connected',
                          task_text_func=None, on_task_click=None) -> Window:
    """Modern runtime header with connection status and prominent task button"""
    def get_header_text():
        text = [
            ('class:title', '❯ CCCC Orchestrator  '),
            (status_color, status_dot),
        ]
        # Add task info with prominent button
        if task_text_func:
            task_text = task_text_func()
            if task_text:
                # Task status info + prominent button
                text.extend([
                    ('', '  │  '),
                    ('class:task-status', task_text),
                    ('', '  '),
                    # Prominent button with background - looks like a real button
                    ('class:button.task.bg', ' ▼ Details '),
                ])
            else:
                # No tasks - just show button
                text.extend([
                    ('', '  │  '),
                    ('class:button.task.bg', ' ▼ Details '),
                ])
        text.extend([
            ('', '\n'),
            ('class:hint', '  /help · PgUp/Dn scroll · T or click [▼ Details] · /quit'),
        ])
        return text
    
    ctrl = FormattedTextControl(lambda: get_header_text(), focusable=False)
    
    # Attach mouse handler for task button click
    if on_task_click:
        original_handler = ctrl.mouse_handler
        def custom_mouse_handler(mouse_event):
            from prompt_toolkit.mouse_events import MouseEventType
            if mouse_event.event_type == MouseEventType.MOUSE_UP:
                # Click on first line toggles task panel
                if mouse_event.position.y == 0:
                    on_task_click()
                    return None
            if original_handler:
                return original_handler(mouse_event)
            return NotImplemented
        ctrl.mouse_handler = custom_mouse_handler
    
    return Window(
        content=ctrl,
        height=Dimension(min=2, max=3),
        dont_extend_height=True,
        style='class:header',
    )


def create_section_header(title: str) -> Window:
    """Section separator"""
    text = [('class:section', f'─── {title} ' + '─' * (40 - len(title)))]
    return Window(content=FormattedTextControl(text), height=1, dont_extend_height=True)




class CCCCSetupApp:
    """Main TUI application"""

    def __init__(self, home: Path):
        self.home = home
        self.config = SetupConfig()
        self.actors_available: List[str] = []
        self.error_msg: str = ''
        self.setup_visible: bool = True
        self.modal_open: bool = False
        self.task_detail_open: bool = False  # Track if Level 2 task detail is open
        self.current_dialog: Optional[Float] = None
        self.dialog_ok_handler: Optional[callable] = None
        self.floats: List[Float] = []  # FloatContainer floats list

        # Dual interaction system state management
        self.focused_option_index: int = 0  # Dynamic: will be updated based on current mode
        self.navigation_items = []  # Will contain all navigable items (buttons and input fields)
        self.config_buttons = {}  # Will be populated with button references

        # Visual feedback state
        self.show_help_hint: bool = True  # Toggle for help hints

        # Command history
        self.command_history: List[str] = []
        self.history_index: int = -1
        self.current_input: str = ''

        # Reverse search state
        self.reverse_search_mode: bool = False
        self.search_query: str = ''
        self.search_results: List[str] = []
        self.search_index: int = 0

        # Actor availability cache: {actor_name: (is_available, hint)}
        self.actor_availability: Dict[str, Tuple[bool, str]] = {}

        # Current configuration values (for display)
        self.current_actor: str = 'claude'
        self.current_foreman: str = 'none'
        self.current_mode: str = 'tmux'

        # Additional availability for foreman
        self.foreman_availability: Dict[str, Dict[str, Any]] = {}

        # Connection state
        self.orchestrator_connected: bool = False
        self.last_update_time: float = 0

        # Value cycling methods flag (deferred until after UI is built)
        self.value_cycling_initialized = False

        # Help hint state
        self.help_hint_visible = True

        # Load config
        self._load_existing_config()

        # Check actor availability after loading actors list
        self._check_actor_availability()

        # Initialize UI components
        self.error_label = Label(text='', style='class:error')
        # Dynamic hint labels for single-peer mode
        is_single_peer = self.config.peerB == 'none'
        self.peerB_hint_label = Label(
            text='ℹ Single-peer mode: Only PeerA will run' if is_single_peer else 'Validation peer (equal)',
            style='class:hint'
        )
        self.foreman_hint_label = Label(
            text='💡 Recommended for single-peer autonomous operation' if (is_single_peer and self.config.foreman == 'none') else 'Background scheduler (self-check/maintenance/compact)',
            style='class:hint'
        )
        self.buttons: List[Button] = []
        self.setup_content = self._build_setup_panel()

        # Initialize Runtime UI after setup UI is built
        ts = time.strftime('%H:%M:%S')
        initial_msg = f"""[{ts}] SYS CCCC Orchestrator
[{ts}] ... Multi-Agent collaboration system
[{ts}] ... Type /help for commands and shortcuts
"""
        self.timeline = TextArea(
            text=initial_msg,
            scrollbar=True,
            read_only=True,
            focusable=True,
            focus_on_click=True,  # Enable click-to-focus for mouse events
            wrap_lines=True  # Enable auto-wrap as fallback for dynamic width handling
        )
        
        # Add custom mouse wheel handler for scrolling
        self._setup_timeline_mouse_scroll()
        
        # Create completer with threading for better responsiveness
        self.command_completer = CommandCompleter()

        self.input_field = TextArea(
            height=1,
            prompt=self._get_dynamic_prompt,
            multiline=False,
            completer=ThreadedCompleter(self.command_completer),
            complete_while_typing=True,
        )
        # Status panel removed - all info now in bottom footer
        self.message_count: int = 0

        # Blueprint task panel (if available)
        self.task_panel = None
        if TaskPanel is not None:
            try:
                # Use project root (parent of .cccc)
                root = self.home.parent if self.home.name == '.cccc' else self.home
                self.task_panel = TaskPanel(root, on_toggle=lambda: self.app.invalidate() if self.app else None)
            except Exception:
                pass

        # Create the application
        self._create_ui()

        # Create application
        self.app = Application(
            layout=self._create_root_layout(),
            key_bindings=self.key_bindings,
            style=self.style,
            full_screen=True,
            mouse_support=True
        )

        # Set initial focus to first button and update visual
        try:
            self.app.layout.focus(self.btn_peerA)
        except Exception:
            pass

        # Update navigation items and visual
        self._update_navigation_items()
        self._update_focus_visual()

        # Show initial help message
        if self.help_hint_visible:
            self._write_timeline("🎉 Welcome to CCCC! Use ↑↓ to navigate, ←→ to change values, Enter for details, F1 to toggle help", 'info')

    def _handle_mode_change(self) -> None:
        """Handle special case when mode is changed"""
        try:
            # Save current IM config before rebuilding
            self._save_im_config()

            # Clear cached input fields to force recreation with new mode
            if hasattr(self, 'token_field'):
                delattr(self, 'token_field')
            if hasattr(self, 'channel_field'):
                delattr(self, 'channel_field')

            # Rebuild setup panel to show/hide IM configuration
            self.setup_content = self._build_setup_panel()

            # Update navigation items list
            self._update_navigation_items()

            # Update root layout content
            if hasattr(self.root, 'content'):
                self.root.content = self._build_root_content()

            # Reinitialize focus to keep current item focused
            if hasattr(self, 'app') and self.app:
                try:
                    if self.focused_option_index < len(self.navigation_items):
                        target_item = self.navigation_items[self.focused_option_index]
                        if target_item['type'] == 'button':
                            self.app.layout.focus(target_item['widget'])
                        elif target_item['type'] == 'input':
                            self.app.layout.focus(target_item['widget'])
                except Exception:
                    pass

            # Invalidate UI to refresh
            try:
                self.app.invalidate()
            except Exception:
                pass

        except Exception:
            pass  # Silently fail to avoid breaking navigation

    def _update_focus_visual(self) -> None:
        """Update visual indication of currently focused option (cursor moves with focus)"""
        try:
            # Update all navigation items based on focus
            for i, item in enumerate(self.navigation_items):
                if i == self.focused_option_index:
                    # Focused item - highlight with GitHub green
                    if item['type'] == 'button':
                        item['widget'].style = "#3fb950 bold"  # GitHub green
                    elif item['type'] == 'input':
                        # Input fields get border style to show focus
                        item['widget'].style = "#c9d1d9 bg:#0d1117"  # High contrast
                else:
                    # Unfocused items - normal styling
                    if item['type'] == 'button':
                        config_name = item['name']
                        if config_name in ['peerA', 'peerB']:
                            current_value = getattr(self.config, config_name, None)
                            if current_value and self.actor_availability.get(current_value, (False, ""))[0]:
                                item['widget'].style = "#58a6ff"  # Blue - configured
                            else:
                                item['widget'].style = "#f85149"  # Red - missing
                        elif config_name == 'aux':
                            current_value = getattr(self.config, 'aux', None)
                            if current_value and current_value != 'none':
                                item['widget'].style = "#58a6ff"  # Blue - configured
                            else:
                                item['widget'].style = "#8b949e"  # Gray - optional
                        elif config_name == 'foreman':
                            current_value = getattr(self.config, 'foreman', None)
                            if current_value and current_value != 'none':
                                item['widget'].style = "#58a6ff"  # Blue - configured
                            else:
                                item['widget'].style = "#8b949e"  # Gray - optional
                        elif config_name == 'mode':
                            item['widget'].style = "#58a6ff"  # Blue - mode indicator
                    elif item['type'] == 'input':
                        # Unfocused input fields
                        item['widget'].style = "#8b949e bg:#161b22"  # Muted background

            # Invalidate UI to refresh
            if hasattr(self, 'app'):
                try:
                    self.app.invalidate()
                except Exception:
                    pass

        except Exception:
            pass  # Silently fail to avoid breaking navigation

    def get_focused_option_display(self, option_name: str, is_focused: bool = False) -> str:
        """Get display text for an option with focus indication"""
        prefix = "▶ " if is_focused else "  "
        return f"{prefix}{option_name}"

    def _create_focused_label(self, text: str, config_index: int) -> Any:
        """Create a label without focus indicator (focus shown by button highlight)"""
        # No triangle prefix - focus is indicated by button color
        return FormattedTextControl([('class:label', text)])

    def _setup_value_cycling_deferred(self) -> None:
        """Initialize value cycling methods after UI is built"""

        def get_value_choices(config_name: str) -> List[str]:
            """Get available values for a config option"""
            if config_name == 'peerA':
                # PeerA: required, only include available actors
                if self.actors_available:
                    return self.actors_available
                else:
                    return ['claude']  # Fallback to claude if no actors found
            elif config_name == 'peerB':
                # PeerB: include 'none' for single-peer mode
                if self.actors_available:
                    return self.actors_available + ['none']
                else:
                    return ['claude', 'none']
            elif config_name == 'aux':
                # Optional: include none and all available actors
                return ['none'] + self.actors_available
            elif config_name == 'foreman':
                # Foreman options
                return ['none', 'reuse_aux'] + self.actors_available
            elif config_name == 'mode':
                # Interaction modes (wecom is beta)
                return ['tmux', 'telegram', 'slack', 'discord', 'wecom']
            elif config_name == 'self_opt':
                # Self-optimization toggle (boolean as list)
                return [True, False]
            else:
                return []

        def cycle_config_value(config_name: str, direction: int = 1) -> None:
            """Cycle config value forward (1) or backward (-1)"""
            choices = get_value_choices(config_name)

            # Special handling for self_opt which uses self_opt_enabled attribute
            if config_name == 'self_opt':
                current_value = self.config.self_opt_enabled
            else:
                current_value = getattr(self.config, config_name, None)

            if not choices:
                return

            # Find current value index
            try:
                current_index = choices.index(current_value) if current_value in choices else 0
            except ValueError:
                current_index = 0

            # Calculate new index with wrap-around
            new_index = (current_index + direction) % len(choices)
            new_value = choices[new_index]

            # Update config (self_opt handled separately below)
            if config_name != 'self_opt':
                setattr(self.config, config_name, new_value)

            # Update button text - use direct button reference
            button = None
            single_peer_ok = False
            if config_name == 'peerA':
                button = self.btn_peerA
                required = True
                none_ok = False
            elif config_name == 'peerB':
                button = self.btn_peerB
                required = True
                none_ok = False
                single_peer_ok = True  # Allow 'none' for single-peer mode
            elif config_name == 'aux':
                button = self.btn_aux
                required = False
                none_ok = True
            elif config_name == 'foreman':
                button = self.btn_foreman
                required = False
                none_ok = True
            elif config_name == 'mode':
                button = self.btn_mode
                required = False
                none_ok = False
            elif config_name == 'self_opt':
                # Handle self_opt toggle (boolean)
                self.config.self_opt_enabled = new_value
                self.btn_self_opt.text = '[●] ON ' if new_value else '[○] OFF'
                self._refresh_ui()
                return  # Early return since we handle everything here

            if button:
                if config_name == 'mode':
                    button.text = f'[●] {new_value}'
                else:
                    button.text = self._format_button_text(new_value, required=required, none_ok=none_ok, single_peer_ok=single_peer_ok)

            # Handle special case: mode change requires UI rebuild
            if config_name == 'mode':
                self._handle_mode_change()

            # Trigger UI refresh
            self._refresh_ui()

        # Store cycling method for use in keyboard bindings
        self.cycle_config_value = cycle_config_value

    def _update_setup_button_text(self):
        """Update setup button text"""
        button_texts = {
            'actor': f"Agent [b]A[/b]: {self.current_actor} ({'✓' if self.actor_availability[self.current_actor]['configured'] else '✗'}{' ' + self.actor_availability[self.current_actor]['version'] if self.actor_availability[self.current_actor]['version'] else ''})",
            'foreman': f"Foreman [b]B[/b]: {self.current_foreman} ({'✓' if self.foreman_availability[self.current_foreman]['configured'] else '✗'}{' ' + self.foreman_availability[self.current_foreman]['version'] if self.foreman_availability[self.current_foreman]['version'] else ''})",
            'mode': f"Mode: {self.current_mode}"
        }

        self.config_buttons['actor'].text = button_texts['actor']
        self.config_buttons['foreman'].text = button_texts['foreman']
        self.config_buttons['mode'].text = button_texts['mode']

        # Update button style based on configuration status
        actor_configured = self.actor_availability[self.current_actor]['configured']
        foreman_configured = self.foreman_availability[self.current_foreman]['configured']

        self.config_buttons['actor'].style = "#3fb950" if actor_configured else "#f85149"  # GitHub colors
        self.config_buttons['foreman'].style = "#3fb950" if foreman_configured else "#f85149"

    def _create_ui(self):
        """Create UI components"""
        self._create_styles()
        self._create_layout()
        self.key_bindings = self._create_key_bindings()
        self._create_dialogs()

    def _build_task_panel_container(self):
        """Build the conditional task panel container with mouse support and adaptive height."""
        
        def get_task_panel_content():
            """Get task panel content with dynamic width."""
            if not self.task_panel:
                return ""
            # Get terminal width (default to 80 if unavailable)
            try:
                width = self.app.output.get_size().columns if self.app else 80
            except Exception:
                width = 80
            # Use 90% of terminal width, min 60, max 120
            panel_width = max(60, min(120, int(width * 0.9)))
            return self.task_panel.get_expanded_text(width=panel_width)
        
        def get_panel_height():
            """Calculate panel height based on display items (tasks + errors)."""
            if not self.task_panel:
                return 8
            
            # Get display items count and error count
            display_count, error_count = self.task_panel.get_display_item_count()
            
            # Layout analysis (line count):
            # 1: Top border
            # 2: Empty line
            # 3: Column headers (if items exist)
            # 4: Separator (if items exist)
            # 5-N: Task/error rows (display_count)
            # N+1: Empty line
            # N+2: Progress line (if items exist)
            # N+3: Error warning line (if error_count > 0)
            # N+4: Help line
            # N+5: Bottom border
            if display_count == 0:
                # No items: top(1) + empty(1) + message(1) + empty(1) + bottom(1) = 5
                return 5
            else:
                # Base: top(1) + empty(1) + header(1) + sep(1) + items + empty(1) + progress(1) + help(1) + bottom(1) = 8 + items
                base_height = 8 + display_count
                if error_count > 0:
                    base_height += 1  # Error warning line
                return min(base_height, 20)  # Cap at 20 to leave room for timeline
        
        # Create FormattedTextControl with mouse handler
        ctrl = FormattedTextControl(
            get_task_panel_content,
            focusable=False,
            show_cursor=False,
        )
        
        # Add mouse handler for task row clicks
        def handle_mouse(mouse_event):
            from prompt_toolkit.mouse_events import MouseEventType
            if mouse_event.event_type == MouseEventType.MOUSE_UP:
                if self.task_panel:
                    # Calculate which row was clicked
                    # Header rows: line 0 (top border), 1 (empty), 2 (column headers), 3 (separator)
                    # Task rows start at line 4
                    row = mouse_event.position.y - 4
                    display_count, _ = self.task_panel.get_display_item_count()
                    
                    if 0 <= row < display_count:
                        # Select the clicked task
                        self.task_panel.selected_index = row
                        # Open task detail dialog (Level 1 stays open)
                        task_id = self.task_panel.get_selected_task_id()
                        if task_id:
                            self._show_task_detail_dialog(task_id)
                        try:
                            self.app.invalidate()
                        except Exception:
                            pass
                        return None  # Consume event
            return NotImplemented
        
        # Attach mouse handler
        ctrl.mouse_handler = handle_mouse
        
        # Window with dynamic height - use Dimension for proper sizing
        return ConditionalContainer(
            content=Window(
                content=ctrl,
                height=lambda: get_panel_height(),
                dont_extend_height=True,
                style='class:task-panel',
            ),
            filter=Condition(lambda: self.task_panel is not None and self.task_panel.expanded)
        )

    def _build_root_content(self):
        """Build the root content HSplit with header, task panel, and setup panel"""
        return HSplit([
            create_header(),
            self._build_task_panel_container(),  # Shows when expanded
            self.setup_content,
        ])

    def _create_root_layout(self):
        """Create the root layout"""
        # Create root container with conditional task panel
        self.root = FloatContainer(
            content=self._build_root_content(),
            floats=[]
        )
        return Layout(self.root)

    def _create_layout(self):
        """Create layout components"""
        # Layout is created in _create_root_layout for now
        pass

    def _create_dialogs(self):
        """Create dialog components"""
        # Dialogs are created as needed
        pass

    def _create_styles(self):
        """Create styles"""
        self.style = Style.from_dict({
            # === 2025 Modern Professional TUI Color Scheme ===
            # Inspired by: GitHub Dark, VS Code Dark+, JetBrains New UI
            
            # Base text colors - High contrast, easy to read
            'title': '#58a6ff bold',                    # GitHub blue - Professional headers
            'title.bold': '#58a6ff bold underline',     # Extra emphasis for titles
            'subtitle': '#8b949e',                      # Muted gray - Secondary text
            'heading': '#f0883e bold',                  # Warm orange - Section headers
            'separator': '#30363d',                     # Subtle gray - Visual breaks
            'version': '#6e7681',                       # Quiet gray - Version info
            'ascii-art': '#58a6ff',                     # Match title blue
            
            # Section markers - Clear hierarchy
            'section': '#58a6ff',                       # Blue for sections
            'section.bold': '#58a6ff bold',             # Bold sections
            
            # Config button styles - Traffic light system
            'config-button': '#c9d1d9',                 # Default: Bright white-gray
            'config-button.focused': '#3fb950 bold',    # Focused: GitHub green
            'config-button.configured': '#58a6ff',      # Configured: Blue indicator
            'config-button.unconfigured': '#f85149',    # Unconfigured: Red warning
            
            # Status indicators - Semantic colors
            'status-indicator': '#3fb950',              # Green for active
            'status-text': '#c9d1d9',                   # High contrast text
            'success': '#3fb950 bold',                  # Success messages
            'success.bold': '#3fb950 bold',             # Success emphasis
            
            # Message type styles - Clear differentiation
            'msg.user': '#ffa657',                      # Warm amber - User input
            'msg.system': '#58a6ff',                    # Blue - System messages
            'msg.error': '#f85149 bold',                # Red - Errors
            'msg.warning': '#d29922',                   # Orange - Warnings
            'msg.info': '#79c0ff',                      # Light blue - Info
            'msg.debug': '#6e7681',                     # Muted gray - Debug
            
            # Status dots - Traffic light semantic
            'status.connected': '#3fb950',              # Green - Connected
            'status.disconnected': '#f85149',           # Red - Disconnected  
            'status.idle': '#6e7681',                   # Gray - Idle
            'status.warning': '#d29922',                # Orange - Warning
            
            # Dialog/Modal components - Better contrast
            'dialog': 'bg:#161b22',                     # Darker background for depth
            'dialog.body': 'bg:#0d1117',                # Even darker for content area
            'dialog.border': '#30363d',                 # Subtle border
            'dialog.frame.label': '#c9d1d9',            # Frame labels - high contrast
            
            # Buttons - Modern flat design with clear states
            'button': 'bg:#21262d #c9d1d9',             # Default: Elevated gray
            'button.focused': 'bg:#238636 #ffffff bold', # Focused: GitHub green
            'button.arrow': '#8b949e',                  # Arrow indicators
            'button-frame': '#58a6ff',                  # Frame around action buttons
            
            # Radio list - Clear selection and proper visibility
            'radio-list': '#c9d1d9',                    # Container - high contrast
            'radio': '#c9d1d9',                         # Default text - high contrast
            'radio-selected': '#3fb950 bold',           # Selected: Green
            'radio-checked': '#58a6ff bold',            # Checked: Blue
            'radio-number': '#8b949e',                  # Numbers - muted
            'radio.focused': '#58a6ff bold',            # Focused: Blue
            'radio.disabled': '#6e7681',                # Disabled: Dark gray
            
            # Completion menu - Polished dropdown
            'completion-menu': 'bg:#161b22 #c9d1d9',              # Menu background
            'completion-menu.completion': 'bg:#161b22 #c9d1d9',   # Item default
            'completion-menu.completion.current': 'bg:#1f6feb #ffffff bold', # Selected item
            'completion-menu.meta': 'bg:#161b22 #8b949e',         # Meta info
            'completion-menu.meta.current': 'bg:#1f6feb #c9d1d9', # Selected meta
            
            # Prompt - Clear command indicator
            'prompt': '#58a6ff bold',                   # Blue - Command prompt
            'prompt.search': '#d29922 bold',            # Orange - Search mode
            
            # Input fields - Clear editing area
            'input-field': '#c9d1d9 bg:#0d1117',        # High contrast input
            'input-frame': '#c9d1d9',                   # Frame border
            
            # Info/Help text - Subtle but readable
            'hint': '#8b949e',                          # Muted gray - Help text (increased contrast)
            'info': '#79c0ff',                          # Light blue - Information
            'value': '#a5d6ff',                         # Bright blue - Values
            
            # Warnings and errors - Clear semantic meaning
            'warning': '#d29922 bold',                  # Orange - Warnings
            'error': '#f85149 bold',                    # Red - Errors
            'critical': '#da3633 bold',                 # Dark red - Critical

            # Task Panel - Professional dark theme with good contrast
            'task-panel': 'bg:#1c2128 #c9d1d9',         # Panel background - slightly lighter than main
            'task-panel.collapsed': 'bg:#161b22',       # Collapsed background
            'task-panel.expanded': 'bg:#1c2128',        # Expanded background - visible separation
            'task-panel.header': '#58a6ff bold',        # Header text
            'task-panel.border': '#3fb950',             # Panel border - green accent
            'task-panel.icon': '#58a6ff',               # Status icons
            'task-panel.progress': '#3fb950',           # Progress numbers
            'task-panel.arrow': '#8b949e',              # Arrow separator
            'task-panel.current': '#ffa657',            # Current task
            'task-panel.step': '#79c0ff',               # Step indicator
            'task-panel.id': '#58a6ff bold',            # Task ID
            'task-panel.name': '#c9d1d9',               # Task name
            'task-panel.selected': 'bg:#2d333b #ffffff bold',  # Selected row - highlighted
            'task-panel.pending-review': '#d29922',     # Pending review - amber
            'task-panel.complete': '#3fb950',           # Complete status
            'task-panel.active': '#ffa657',             # Active status
            'task-panel.planned': '#8b949e',            # Planned status
            'task-panel.hint': '#8b949e',               # Hint text - more visible
            'task-panel.empty': '#8b949e italic',       # Empty state
            
            # Task Detail Dialog - High contrast for readability
            'task-detail': 'bg:#1c2128 #e6edf3',        # Light text on dark bg
            'task-detail.title': '#58a6ff bold',        # Title - blue
            'task-detail.label': '#8b949e',             # Labels - muted
            'task-detail.value': '#e6edf3',             # Values - bright white
            'task-detail.step-done': '#3fb950',         # Done steps - green
            'task-detail.step-active': '#ffa657 bold',  # Active step - orange
            'task-detail.step-pending': '#6e7681',      # Pending steps - gray
            
            # Task status in header (before button)
            'task-status': '#79c0ff',                     # Task status info
            
            # Task Button in Header - PROMINENT with background
            'button.task.bg': 'bg:#238636 #ffffff bold',  # Green background, white text - OBVIOUS BUTTON
        })

    def _load_existing_config(self) -> None:
        """Load configuration from yaml files"""
        # Load actors
        agents_data = _load_yaml(self.home, 'settings/agents.yaml')
        acts = agents_data.get('actors') or {}
        self.actors_available = list(acts.keys()) if isinstance(acts, dict) else []

        if not self.actors_available:
            self.actors_available = ['claude', 'codex', 'gemini', 'droid', 'opencode']

        # Load roles and mode from cli_profiles.yaml
        cli_profiles = _load_yaml(self.home, 'settings/cli_profiles.yaml')
        roles = cli_profiles.get('roles') or {}
        self.config.peerA = str((roles.get('peerA') or {}).get('actor') or '')
        # PeerB: handle 'none' explicitly for single-peer mode
        peerB_actor = str((roles.get('peerB') or {}).get('actor') or '')
        # Normalize: empty string from missing section means check if section existed
        if 'peerB' in roles and peerB_actor.lower() == 'none':
            self.config.peerB = 'none'  # Explicit single-peer mode
        else:
            self.config.peerB = peerB_actor
        self.config.aux = str((roles.get('aux') or {}).get('actor') or 'none')

        # Load mode selection (im_mode field)
        saved_mode = cli_profiles.get('im_mode', 'tmux')
        if saved_mode in ['tmux', 'telegram', 'slack', 'discord']:
            self.config.mode = saved_mode
        else:
            self.config.mode = 'tmux'

        # Smart defaults for roles (only apply if not explicitly configured)
        if not self.config.peerA and len(self.actors_available) > 0:
            self.config.peerA = self.actors_available[0]
        # PeerB: only apply default if NOT explicitly set to 'none' (single-peer mode)
        if not self.config.peerB and self.config.peerB != 'none':
            if len(self.actors_available) > 1:
                self.config.peerB = self.actors_available[1]
            elif len(self.actors_available) > 0:
                self.config.peerB = self.actors_available[0]

        # Load foreman
        foreman_data = _load_yaml(self.home, 'settings/foreman.yaml')
        self.config.foreman = str(foreman_data.get('agent') or 'none')
        
        # Load self-optimization setting
        self_opt = foreman_data.get('self_optimization', {})
        if isinstance(self_opt, dict):
            self.config.self_opt_enabled = bool(self_opt.get('enabled', False))
        else:
            self.config.self_opt_enabled = False

        # Load IM configurations
        # Telegram
        telegram_data = _load_yaml(self.home, 'settings/telegram.yaml')
        self.config.tg_token = str(telegram_data.get('token') or '')
        chats = telegram_data.get('allow_chats') or []
        if isinstance(chats, list) and chats:
            self.config.tg_chat = str(chats[0])

        # Slack
        slack_data = _load_yaml(self.home, 'settings/slack.yaml')
        self.config.sl_bot_token = str(slack_data.get('bot_token') or '')
        self.config.sl_app_token = str(slack_data.get('app_token') or '')
        channels = (slack_data.get('channels') or {}).get('to_user') or []
        if isinstance(channels, list) and channels:
            self.config.sl_chan = str(channels[0])

        # Discord
        discord_data = _load_yaml(self.home, 'settings/discord.yaml')
        self.config.dc_token = str(discord_data.get('bot_token') or '')
        channels = (discord_data.get('channels') or {}).get('to_user') or []
        if isinstance(channels, list) and channels:
            self.config.dc_chan = str(channels[0])

        # WeCom (outbound-only)
        wecom_data = _load_yaml(self.home, 'settings/wecom.yaml')
        self.config.wc_webhook = str(wecom_data.get('webhook_url') or '')

    def _check_actor_availability(self) -> None:
        """Check availability of all known actors"""
        # Handle empty actors list
        if not self.actors_available:
            self.actors_available = ['claude', 'codex', 'gemini']

        # Check all actors in actors_available list
        for actor in self.actors_available:
            try:
                available, hint = check_actor_available(actor, self.home)
                self.actor_availability[actor] = (available, hint)
            except Exception:
                # Fallback: mark as unknown
                self.actor_availability[actor] = (False, "Check failed")

        # Also check 'none' for optional actors
        self.actor_availability['none'] = (True, "Disabled")

    def _build_setup_panel(self) -> HSplit:
        """Build compact setup panel (8-char labels)"""
        # Build buttons
        btn_peerA = Button(
            text=self._format_button_text(self.config.peerA, required=True),
            handler=lambda: self._show_actor_dialog('peerA'),
            width=20,
            left_symbol='',
            right_symbol=''
        )
        btn_peerB = Button(
            text=self._format_button_text(self.config.peerB, required=True, single_peer_ok=True),
            handler=lambda: self._show_actor_dialog('peerB'),
            width=20,
            left_symbol='',
            right_symbol=''
        )
        btn_aux = Button(
            text=self._format_button_text(self.config.aux, none_ok=True),
            handler=lambda: self._show_actor_dialog('aux'),
            width=20,
            left_symbol='',
            right_symbol=''
        )
        btn_foreman = Button(
            text=self._format_button_text(self.config.foreman, none_ok=True),
            handler=self._show_foreman_dialog,
            width=20,
            left_symbol='',
            right_symbol=''
        )
        btn_mode = Button(
            text=f'[●] {self.config.mode}',
            handler=self._show_mode_dialog,
            width=20,
            left_symbol='',
            right_symbol=''
        )
        btn_confirm = Button(
            text='🚀 Launch',
            handler=self._confirm_and_launch,
            width=12,
            left_symbol='',
            right_symbol=''
        )
        btn_quit = Button(
            text='❌ Quit',
            handler=self._quit_app,
            width=12,
            left_symbol='',
            right_symbol=''
        )

        # Store button references
        self.btn_peerA = btn_peerA
        self.btn_peerB = btn_peerB
        self.btn_aux = btn_aux
        self.btn_foreman = btn_foreman
        self.btn_mode = btn_mode
        self.btn_confirm = btn_confirm
        self.btn_quit = btn_quit
        
        # Self-optimization toggle button (ON/OFF)
        def toggle_self_opt():
            self.config.self_opt_enabled = not self.config.self_opt_enabled
            self.btn_self_opt.text = '[●] ON ' if self.config.self_opt_enabled else '[○] OFF'
            self._refresh_ui()

        btn_self_opt = Button(
            text='[●] ON ' if self.config.self_opt_enabled else '[○] OFF',
            handler=toggle_self_opt,
            width=20,
            left_symbol='',
            right_symbol=''
        )
        self.btn_self_opt = btn_self_opt

        # Map config names to buttons for easy access
        self.config_buttons = {
            'peerA': btn_peerA,
            'peerB': btn_peerB,
            'aux': btn_aux,
            'foreman': btn_foreman,
            'self_opt': btn_self_opt,  # Add self-opt to navigation
            'mode': btn_mode
        }

        # Build initial buttons list (will be updated dynamically)
        # Include self_opt button for keyboard navigation
        self.buttons = [btn_peerA, btn_peerB, btn_aux, btn_foreman, btn_self_opt, btn_mode]
        self._update_navigation_items()

        # Clean, minimal layout with dual interaction system
        items = [
            self.error_label,

            # Core agents
            create_section_header('Core Agents'),
            VSplit([
                Window(width=10, content=self._create_focused_label('PeerA', 0)),
                btn_peerA,
                Window(width=2),
                Label(text='Strategic peer (equal)', style='class:hint'),
            ], padding=1),
            VSplit([
                Window(width=10, content=self._create_focused_label('PeerB', 1)),
                btn_peerB,
                Window(width=2),
                self.peerB_hint_label,
            ], padding=1),
            Window(height=1),

            # Optional agents
            create_section_header('Optional'),
            VSplit([
                Window(width=10, content=self._create_focused_label('Aux', 2)),
                btn_aux,
                Window(width=2),
                Label(text='Optional burst capacity (heavy reviews/tests/transforms)', style='class:hint'),
            ], padding=1),
            VSplit([
                Window(width=10, content=self._create_focused_label('Foreman', 3)),
                btn_foreman,
                Window(width=2),
                self.foreman_hint_label,
            ], padding=1),
            VSplit([
                Window(width=10, content=self._create_focused_label('Self-Opt', 4)),
                btn_self_opt,
                Window(width=2),
                Label(text='Auto performance detection & optimization suggestions[beta]', style='class:hint'),
            ], padding=1),
            Window(height=1),

            # Interaction mode
            create_section_header('Mode'),
            VSplit([
                Window(width=10, content=self._create_focused_label('Connect', 5)),
                btn_mode,
                Window(width=2),
                Label(text='Interaction mode (tmux / Telegram / Slack / Discord / WeCom[beta])', style='class:hint'),
            ], padding=1),
        ]

        # IM Configuration (integrated approach)
        if self.config.mode in ('telegram', 'slack', 'discord', 'wecom'):
            items.extend([
                Window(height=1),
                create_section_header('IM Configuration'),
            ])

            if self.config.mode == 'slack':
                # Slack needs both bot and app tokens
                items.extend([
                    # Bot Token input
                    VSplit([
                        Window(width=20, content=FormattedTextControl('Bot Token (xoxb-):')),
                        self._create_token_field(),
                        Window(width=2),
                        Label(text='Outbound messages', style='class:hint'),
                    ], padding=1),

                    # App Token input
                    VSplit([
                        Window(width=20, content=FormattedTextControl('App Token (xapp-):')),
                        self._create_app_token_field(),
                        Window(width=2),
                        Label(text='Inbound messages', style='class:hint'),
                    ], padding=1),

                    # Channel ID input
                    VSplit([
                        Window(width=12, content=self._create_channel_label()),
                        self._create_channel_field(),
                        Window(width=2),
                        Label(text=self._get_channel_hint(), style='class:hint'),
                    ], padding=1),

                    # Help text
                    Window(height=1),
                    Label(text='Both tokens required for bidirectional communication', style='class:hint'),
                ])
            elif self.config.mode == 'wecom':
                # WeCom only needs webhook URL (outbound-only)
                items.extend([
                    # Webhook URL input
                    VSplit([
                        Window(width=14, content=FormattedTextControl('Webhook URL:')),
                        self._create_webhook_field(),
                        Window(width=2),
                        Label(text='WeCom robot webhook', style='class:hint'),
                    ], padding=1),

                    # Help text
                    Window(height=1),
                    Label(text='⚠ Outbound-only (no inbound support). Get URL from WeCom group robot settings.', style='class:hint'),
                    Label(text='[BETA] This bridge is in beta testing.', style='class:warning'),
                ])
            else:
                # Telegram and Discord only need bot token
                items.extend([
                    # Bot Token input
                    VSplit([
                        Window(width=10, content=FormattedTextControl('Bot Token:')),
                        self._create_token_field(),
                        Window(width=2),
                        Label(text='Required for bot authentication', style='class:hint'),
                    ], padding=1),

                    # Channel/Chat ID input
                    VSplit([
                        Window(width=10, content=self._create_channel_label()),
                        self._create_channel_field(),
                        Window(width=2),
                        Label(text=self._get_channel_hint(), style='class:hint'),
                    ], padding=1),
                ])

        items.extend([
            Window(height=1),
            # Navigation hints
            Window(
                content=FormattedTextControl([
                    ('class:section', '─' * 72 + '\n'),
                    ('class:hint', '   ↑↓ Navigate    ←→ Change Value    Enter Details    Tab Cycle Buttons\n'),
                    ('class:section', '─' * 72),
                ]),
                height=3,
                dont_extend_height=True
            ),
            Window(height=1),
            # Action buttons (left-aligned with small indent, adapts to narrow windows)
            VSplit([
                Window(width=18),  # Small fixed left margin
                Frame(btn_confirm, style='class:button-frame'),
                Window(width=4),  # Gap between buttons
                Frame(btn_quit, style='class:button-frame'),
                Window(),  # Flexible right padding
            ]),
            Window(height=1),
        ])

        # Initialize value cycling methods after UI is built
        try:
            self._setup_value_cycling_deferred()
            self.value_cycling_initialized = True
        except Exception:
            pass  # Ignore errors in value cycling setup

        # Wrap in ScrollablePane for small terminal support
        return ScrollablePane(HSplit(items))

    def _update_navigation_items(self) -> None:
        """Update navigation items list based on current mode"""
        self.navigation_items = []

        # Always include core buttons
        self.navigation_items.extend([
            {'type': 'button', 'name': 'peerA', 'widget': self.btn_peerA},
            {'type': 'button', 'name': 'peerB', 'widget': self.btn_peerB},
            {'type': 'button', 'name': 'aux', 'widget': self.btn_aux},
            {'type': 'button', 'name': 'foreman', 'widget': self.btn_foreman},
            {'type': 'button', 'name': 'self_opt', 'widget': self.btn_self_opt},
            {'type': 'button', 'name': 'mode', 'widget': self.btn_mode},
        ])

        # Add IM input fields if IM mode is selected
        if self.config.mode in ('telegram', 'slack', 'discord'):
            if self.config.mode == 'slack':
                # Slack needs both bot and app tokens in correct order
                self.navigation_items.extend([
                    {'type': 'input', 'name': 'token', 'widget': self._create_token_field()},
                    {'type': 'input', 'name': 'app_token', 'widget': self._create_app_token_field()},
                    {'type': 'input', 'name': 'channel', 'widget': self._create_channel_field()},
                ])
            else:
                # Telegram and Discord only need bot token
                self.navigation_items.extend([
                    {'type': 'input', 'name': 'token', 'widget': self._create_token_field()},
                    {'type': 'input', 'name': 'channel', 'widget': self._create_channel_field()},
                ])

        # Always add Launch and Quit buttons at the end
        self.navigation_items.extend([
            {'type': 'button', 'name': 'confirm', 'widget': self.btn_confirm},
            {'type': 'button', 'name': 'quit', 'widget': self.btn_quit},
        ])

        # Ensure focused index is valid
        if self.focused_option_index >= len(self.navigation_items):
            self.focused_option_index = 0

    def _create_token_field(self):
        """Create bot token input field"""
        if not hasattr(self, 'token_field'):
            mode = self.config.mode
            initial_text = ''
            if mode == 'telegram':
                initial_text = self.config.tg_token or ''
            elif mode == 'slack':
                initial_text = self.config.sl_bot_token or ''
            elif mode == 'discord':
                initial_text = self.config.dc_token or ''

            self.token_field = TextArea(
                height=1,
                multiline=False,
                text=initial_text,
                wrap_lines=False,
                style='class:input-field'
            )
            
            # Add mouse handler to enable clicking to focus
            original_handler = self.token_field.window.content.mouse_handler
            def mouse_handler(mouse_event):
                from prompt_toolkit.mouse_events import MouseEventType
                if mouse_event.event_type == MouseEventType.MOUSE_UP:
                    try:
                        self.app.layout.focus(self.token_field)
                    except Exception:
                        pass
                # Pass through to original handler for text selection
                if original_handler:
                    return original_handler(mouse_event)
            self.token_field.window.content.mouse_handler = mouse_handler

        return self.token_field

    def _create_app_token_field(self):
        """Create app token input field for Slack"""
        if not hasattr(self, 'app_token_field'):
            mode = self.config.mode
            initial_text = ''
            if mode == 'slack':
                initial_text = self.config.sl_app_token or ''

            self.app_token_field = TextArea(
                height=1,
                multiline=False,
                text=initial_text,
                wrap_lines=False,
                style='class:input-field'
            )

            # Add mouse handler to enable clicking to focus
            original_handler = self.app_token_field.window.content.mouse_handler
            def mouse_handler(mouse_event):
                from prompt_toolkit.mouse_events import MouseEventType
                if mouse_event.event_type == MouseEventType.MOUSE_UP:
                    try:
                        self.app.layout.focus(self.app_token_field)
                    except Exception:
                        pass
                # Pass through to original handler for text selection
                if original_handler:
                    return original_handler(mouse_event)
                return None

            self.app_token_field.window.content.mouse_handler = mouse_handler

        return self.app_token_field

    def _create_channel_field(self):
        """Create channel/chat ID input field"""
        if not hasattr(self, 'channel_field'):
            mode = self.config.mode
            initial_text = ''
            if mode == 'telegram':
                initial_text = self.config.tg_chat or ''
            elif mode == 'slack':
                initial_text = self.config.sl_chan or ''
            elif mode == 'discord':
                initial_text = self.config.dc_chan or ''

            self.channel_field = TextArea(
                height=1,
                multiline=False,
                text=initial_text,
                wrap_lines=False,
                style='class:input-field'
            )
            
            # Add mouse handler to enable clicking to focus
            original_handler = self.channel_field.window.content.mouse_handler
            def mouse_handler(mouse_event):
                from prompt_toolkit.mouse_events import MouseEventType
                if mouse_event.event_type == MouseEventType.MOUSE_UP:
                    try:
                        self.app.layout.focus(self.channel_field)
                    except Exception:
                        pass
                # Pass through to original handler for text selection
                if original_handler:
                    return original_handler(mouse_event)
            self.channel_field.window.content.mouse_handler = mouse_handler
            
        return self.channel_field

    def _create_channel_label(self):
        """Create appropriate label for channel/chat ID"""
        mode = self.config.mode
        if mode == 'telegram':
            return FormattedTextControl('Chat ID:')
        elif mode == 'slack':
            return FormattedTextControl('Channel ID:')
        elif mode == 'discord':
            return FormattedTextControl('Channel ID:')
        else:
            return FormattedTextControl('Channel ID:')

    def _create_webhook_field(self):
        """Create WeCom webhook URL input field"""
        if not hasattr(self, 'webhook_field'):
            initial_text = self.config.wc_webhook or ''
            
            self.webhook_field = TextArea(
                height=1,
                multiline=False,
                text=initial_text,
                wrap_lines=False,
                style='class:input-field'
            )
            
            # Add mouse handler to enable clicking to focus
            original_handler = self.webhook_field.window.content.mouse_handler
            def mouse_handler(mouse_event):
                from prompt_toolkit.mouse_events import MouseEventType
                if mouse_event.event_type == MouseEventType.MOUSE_UP:
                    try:
                        self.app.layout.focus(self.webhook_field)
                    except Exception:
                        pass
                if original_handler:
                    return original_handler(mouse_event)
            self.webhook_field.window.content.mouse_handler = mouse_handler
            
        return self.webhook_field

    def _get_channel_hint(self):
        """Get appropriate hint for channel/chat ID"""
        mode = self.config.mode
        if mode == 'telegram':
            return 'Optional: leave blank for auto-discovery'
        elif mode == 'slack':
            return 'Optional: Slack channel ID or workspace ID'
        elif mode == 'discord':
            return 'Optional: Discord channel ID or server ID'
        else:
            return 'Channel identifier'

    def _save_im_config(self):
        """Save IM configuration from input fields"""
        if hasattr(self, 'token_field') and hasattr(self, 'channel_field'):
            mode = self.config.mode
            if mode == 'telegram':
                self.config.tg_token = self.token_field.text.strip()
                self.config.tg_chat = self.channel_field.text.strip()
            elif mode == 'slack':
                self.config.sl_bot_token = self.token_field.text.strip()
                self.config.sl_chan = self.channel_field.text.strip()
                # Also save app token if it exists
                if hasattr(self, 'app_token_field'):
                    self.config.sl_app_token = self.app_token_field.text.strip()
            elif mode == 'discord':
                self.config.dc_token = self.token_field.text.strip()
                self.config.dc_chan = self.channel_field.text.strip()
            elif mode == 'wecom':
                # WeCom only has webhook URL
                if hasattr(self, 'webhook_field'):
                    self.config.wc_webhook = self.webhook_field.text.strip()

    def _format_button_text(self, value: str, required: bool = False, none_ok: bool = False, single_peer_ok: bool = False) -> str:
        """Format button text (no availability status in setup panel)"""
        if not value:
            return '[○] (not set)' if required else '[○] none'
        if value == 'none':
            if none_ok or single_peer_ok:
                return '[○] none'

        # Just show the value without availability indicator
        return f'[●] {value}'

    def _get_provider_summary(self) -> str:
        """Provider summary"""
        mode = self.config.mode
        if mode == 'telegram':
            tok = '●' if self.config.tg_token else '○'
            return f'[{tok}] token set' if self.config.tg_token else '[○] not configured'
        elif mode == 'slack':
            bot_tok = '●' if self.config.sl_bot_token else '○'
            app_tok = '●' if self.config.sl_app_token else '○'
            if self.config.sl_bot_token and self.config.sl_app_token:
                return f'[{bot_tok}] both tokens ready'
            elif self.config.sl_bot_token:
                return f'[{bot_tok}] bot token only'
            else:
                return '[○] not configured'
        elif mode == 'discord':
            tok = '●' if self.config.dc_token else '○'
            return f'[{tok}] token set' if self.config.dc_token else '[○] not configured'
        elif mode == 'wecom':
            wh = '●' if self.config.wc_webhook else '○'
            return f'[{wh}] webhook set [beta]' if self.config.wc_webhook else '[○] not configured [beta]'
        return 'Configure...'

    def _update_buttons_list(self) -> None:
        """Update the navigable buttons list based on current mode"""
        # Start with core buttons
        self.buttons = [
            self.btn_peerA,
            self.btn_peerB,
            self.btn_aux,
            self.btn_foreman,
            self.btn_mode,
        ]

        # Provider buttons removed - integrated approach now
        # IM configuration fields are directly displayed, not separate buttons

        # Always add Launch and Quit buttons at the end
        self.buttons.append(self.btn_confirm)
        self.buttons.append(self.btn_quit)

    def _refresh_ui(self) -> None:
        """Refresh UI"""
        self.btn_peerA.text = self._format_button_text(self.config.peerA, required=True)
        self.btn_peerB.text = self._format_button_text(self.config.peerB, required=True, single_peer_ok=True)
        self.btn_aux.text = self._format_button_text(self.config.aux, none_ok=True)
        self.btn_foreman.text = self._format_button_text(self.config.foreman, none_ok=True)
        self.btn_mode.text = f'[●] {self.config.mode}'

        # Update single-peer mode hint labels
        is_single_peer = self.config.peerB == 'none'
        if hasattr(self, 'peerB_hint_label'):
            if is_single_peer:
                self.peerB_hint_label.text = 'ℹ Single-peer mode: Only PeerA will run'
            else:
                self.peerB_hint_label.text = 'Validation peer (equal)'
        if hasattr(self, 'foreman_hint_label'):
            if is_single_peer and self.config.foreman == 'none':
                self.foreman_hint_label.text = '💡 Recommended for single-peer autonomous operation'
            else:
                self.foreman_hint_label.text = 'Background scheduler (self-check/maintenance/compact)'

        if hasattr(self, 'btn_provider'):
            self.btn_provider.text = self._get_provider_summary()

        if self.error_msg:
            self.error_label.text = f'🚨  {self.error_msg}'
        else:
            self.error_label.text = ''

        try:
            self.app.invalidate()
        except Exception:
            pass

    def _show_actor_dialog(self, role: str) -> None:
        """Show actor dialog using built-in radiolist_dialog"""
        if self.modal_open:
            return

        # Find longest actor name for proper alignment
        max_name_len = max(len(a) for a in self.actors_available)
        max_name_len = max(max_name_len, 4)  # At least 4 for 'none'

        # Build choices - ONLY include available actors
        choices = []
        unavailable_actors = []  # Track unavailable for info message
        
        for actor in self.actors_available:
            available, hint = self.actor_availability.get(actor, (True, "Unknown"))
            if available:
                # Installed: show checkmark and add to choices
                display_text = f'  {actor.ljust(max_name_len)}  ✓  {hint}'
                choices.append((actor, display_text))
            else:
                # Not installed: track but don't add to choices
                unavailable_actors.append(f'{actor}: {hint}')

        # Add 'none' option for optional roles (aux) and peerB (single-peer mode)
        if role == 'aux':
            choices.insert(0, ('none', f'  {"none".ljust(max_name_len)}  -  Disabled'))
        elif role == 'peerB':
            choices.insert(0, ('none', f'  {"none".ljust(max_name_len)}  →  Single-peer mode'))

        # Better title formatting
        role_titles = {
            'peerA': 'Select PeerA',
            'peerB': 'Select PeerB',
            'aux': 'Select Aux Agent',
        }
        title = role_titles.get(role, f'Select {role.upper()}')

        # Get current value
        current = getattr(self.config, role, '')

        # Use the reliable manual dialog implementation
        self._show_actor_dialog_fallback(role, choices, title, current, unavailable_actors)

    def _show_actor_dialog_fallback(self, role: str, choices, title: str, current: str, unavailable_actors: list) -> None:
        """Simple but effective: Standard dialog with clear UI flow"""

        def on_ok() -> None:
            """Called when user clicks OK"""
            setattr(self.config, role, radio.current_value)
            self._close_dialog()
            self._refresh_ui()

        def on_cancel() -> None:
            """Called when user clicks Cancel"""
            self._close_dialog()

        # Create standard RadioList
        radio = RadioList(choices)
        if current and current in [c[0] for c in choices]:
            radio.current_value = current

        # Simple keybindings (just Esc)
        kb_body = KeyBindings()
        @kb_body.add('escape')
        def _escape(event):
            on_cancel()

        # Build dialog body with info about unavailable actors
        body_widgets = [
            Label(text='✓ = Installed and ready to use', style='class:hint'),
        ]
        
        if unavailable_actors:
            body_widgets.append(Label(text='', style=''))  # Spacing
            body_widgets.append(Label(text='⚠ Unavailable actors (not shown in list):', style='class:warning'))
            for unavail in unavailable_actors[:3]:  # Show max 3
                body_widgets.append(Label(text=f'  ✗ {unavail}', style='class:hint'))
            if len(unavailable_actors) > 3:
                body_widgets.append(Label(text=f'  ... and {len(unavailable_actors) - 3} more', style='class:hint'))
        
        body_widgets.append(Label(text='', style=''))  # Spacing
        body_widgets.append(Label(text='↑↓: Navigate  |  Space: select  |  Tab: to buttons  |  Enter: confirm', style='class:hint'))
        body_widgets.append(Window(height=1))
        body_widgets.append(radio)

        dialog = Dialog(
            title=title,
            body=HSplit(body_widgets, key_bindings=kb_body),
            buttons=[
                Button('OK', handler=on_ok),
                Button('Cancel', handler=on_cancel)
            ],
            width=Dimension(min=70, max=90, preferred=80),
            modal=True
        )

        self._open_dialog(dialog, on_ok)
        try:
            self.app.layout.focus(radio)
        except Exception:
            pass

    def _show_foreman_dialog(self) -> None:
        """Show foreman dialog using standard interaction"""
        if self.modal_open:
            return

        # Find longest actor name for proper alignment (same as aux dialog)
        max_name_len = max(len(a) for a in self.actors_available)
        max_name_len = max(max_name_len, 10)  # At least 10 for 'reuse_aux'

        # Build choices - ONLY include available actors
        choices = [
            ('none', f'  {"none".ljust(max_name_len)}  -  Disabled'),
            ('reuse_aux', f'  {"reuse_aux".ljust(max_name_len)}  →  Use same as Aux agent'),
        ]
        
        unavailable_actors = []  # Track unavailable for info message
        
        # Add actors with availability check
        for actor in self.actors_available:
            available, hint = self.actor_availability.get(actor, (True, "Unknown"))
            if available:
                # Installed: show checkmark and add to choices
                display_text = f'  {actor.ljust(max_name_len)}  ✓  {hint}'
                choices.append((actor, display_text))
            else:
                # Not installed: track but don't add to choices
                unavailable_actors.append(f'{actor}: {hint}')

        def on_ok() -> None:
            """Called when user clicks OK"""
            self.config.foreman = radio.current_value
            self._close_dialog()
            self._refresh_ui()

        def on_cancel() -> None:
            """Called when user clicks Cancel"""
            self._close_dialog()

        # Create standard RadioList
        radio = RadioList(choices)
        if self.config.foreman in [c[0] for c in choices]:
            radio.current_value = self.config.foreman

        # Simple keybindings (just Esc)
        kb_body = KeyBindings()
        @kb_body.add('escape')
        def _escape(event):
            on_cancel()

        # Build dialog body with info
        body_widgets = [
            Label(text='Select foreman agent for scheduled tasks', style='class:info'),
            Label(text='✓ = Installed    → = Special option', style='class:hint'),
        ]
        
        if unavailable_actors:
            body_widgets.append(Label(text='', style=''))  # Spacing
            body_widgets.append(Label(text='⚠ Unavailable actors (not shown in list):', style='class:warning'))
            for unavail in unavailable_actors[:3]:  # Show max 3
                body_widgets.append(Label(text=f'  ✗ {unavail}', style='class:hint'))
            if len(unavailable_actors) > 3:
                body_widgets.append(Label(text=f'  ... and {len(unavailable_actors) - 3} more', style='class:hint'))
        
        body_widgets.append(Label(text='', style=''))  # Spacing
        body_widgets.append(Label(text='↑↓: Navigate  |  Space: select  |  Tab: to buttons  |  Enter: confirm', style='class:hint'))
        body_widgets.append(Window(height=1))
        body_widgets.append(radio)

        dialog = Dialog(
            title='Foreman Agent',
            body=HSplit(body_widgets, key_bindings=kb_body),
            buttons=[
                Button('OK', handler=on_ok),
                Button('Cancel', handler=on_cancel)
            ],
            width=Dimension(min=70, max=90, preferred=80),
            modal=True
        )

        self._open_dialog(dialog, on_ok)
        try:
            self.app.layout.focus(radio)
        except Exception:
            pass

    def _show_mode_dialog(self) -> None:
        """Show mode dialog"""
        if self.modal_open:
            return

        choices = [
            ('tmux', 'tmux only'),
            ('telegram', 'Telegram'),
            ('slack', 'Slack'),
            ('discord', 'Discord'),
        ]

        def on_ok() -> None:
            """Called when user clicks OK"""
            old_mode = self.config.mode
            self.config.mode = radio.current_value

            # Rebuild if mode changed
            if old_mode != self.config.mode:
                self.setup_content = self._build_setup_panel()
                self.root.content = self._build_root_content()
                # Update buttons list after mode change
                self._update_buttons_list()
                try:
                    self.app.layout.focus(self.btn_peerA)
                except Exception:
                    pass

            self._close_dialog()
            self._refresh_ui()

        def on_cancel() -> None:
            """Called when user clicks Cancel"""
            self._close_dialog()

        # Create standard RadioList
        radio = RadioList(choices)
        radio.current_value = self.config.mode

        # Simple keybindings (just Esc)
        kb_body = KeyBindings()
        @kb_body.add('escape')
        def _escape(event):
            on_cancel()

        dialog = Dialog(
            title='Interaction Mode',
            body=HSplit([
                Label(text='How to interact with CCCC?', style='class:hint'),
                Label(text='↑↓: Navigate  |  Space: select  |  Tab: to buttons  |  Enter: confirm', style='class:hint'),
                Window(height=1),
                radio,
            ], key_bindings=kb_body),
            buttons=[
                Button('OK', handler=on_ok),
                Button('Cancel', handler=on_cancel)
            ],
            width=Dimension(min=60, max=80, preferred=70),
            modal=True
        )

        self._open_dialog(dialog, on_ok)
        try:
            self.app.layout.focus(radio)
        except Exception:
            pass

    def _show_provider_dialog(self) -> None:
        """Show provider dialog"""
        if self.modal_open:
            return

        mode = self.config.mode
        if mode == 'telegram':
            token_field = TextArea(height=1, multiline=False, text=self.config.tg_token)
            chat_field = TextArea(height=1, multiline=False, text=self.config.tg_chat)

            def on_ok() -> None:
                self.config.tg_token = token_field.text.strip()
                self.config.tg_chat = chat_field.text.strip()
                self._close_dialog()
                self._refresh_ui()

            # Create keybindings for the dialog body
            kb_body = KeyBindings()

            @kb_body.add('escape')
            def _escape(event):
                self._close_dialog()

            # Note: Enter in TextArea is for input, use buttons to save
            dialog = Dialog(
                title='Telegram Configuration',
                body=HSplit([
                    Label(text='Bot Token:'),
                    token_field,
                    Window(height=1),
                    Label(text='Chat ID (optional, leave blank for auto-discovery):'),
                    chat_field,
                ], key_bindings=kb_body),
                buttons=[
                    Button('Save (Enter)', handler=on_ok),
                    Button('Cancel (Esc)', handler=self._close_dialog)
                ],
                width=Dimension(min=60, max=90, preferred=75),
                modal=True
            )

            self._open_dialog(dialog, on_ok)
            try:
                self.app.layout.focus(token_field)
            except Exception:
                pass

        elif mode == 'slack':
            # Create separate fields for bot and app tokens
            bot_token_field = TextArea(
                height=1,
                multiline=False,
                text=self.config.sl_bot_token,
                style='class:input-field'
            )
            app_token_field = TextArea(
                height=1,
                multiline=False,
                text=self.config.sl_app_token,
                style='class:input-field'
            )
            channel_field = TextArea(
                height=1,
                multiline=False,
                text=self.config.sl_chan,
                style='class:input-field'
            )

            def on_ok() -> None:
                # Validate token formats
                bot_token = bot_token_field.text.strip()
                app_token = app_token_field.text.strip()

                # Bot token validation (xoxb- format)
                if bot_token and not bot_token.startswith('xoxb-'):
                    self._write_timeline("Warning: Bot token should start with 'xoxb-'", 'warning')

                # App token validation (xapp- format)
                if app_token and not app_token.startswith('xapp-'):
                    self._write_timeline("Warning: App token should start with 'xapp-'", 'warning')

                self.config.sl_bot_token = bot_token
                self.config.sl_app_token = app_token
                self.config.sl_chan = channel_field.text.strip()
                self._close_dialog()
                self._refresh_ui()

            # Create keybindings for the dialog body
            kb_body = KeyBindings()

            @kb_body.add('escape')
            def _escape(event):
                self._close_dialog()

            dialog = Dialog(
                title='Slack Configuration',
                body=HSplit([
                    # Bot token section
                    Label(text='Bot Token (xoxb-):'),
                    bot_token_field,
                    Window(height=1),
                    # App token section
                    Label(text='App Token (xapp-):'),
                    app_token_field,
                    Window(height=1),
                    # Channel ID section
                    Label(text='Channel ID:'),
                    channel_field,
                    Window(height=1),
                    # Help text
                    Label(text='Bot token: Required for sending messages (outbound)'),
                    Label(text='App token: Optional, enables receiving messages (inbound)'),
                    Label(text='Format: xoxb- for bot token, xapp- for app token'),
                ], key_bindings=kb_body),
                buttons=[
                    Button('Save (Enter)', handler=on_ok),
                    Button('Cancel (Esc)', handler=self._close_dialog)
                ],
                width=Dimension(min=70, max=100, preferred=85),
                modal=True
            )

            self._open_dialog(dialog, on_ok)
            try:
                self.app.layout.focus(bot_token_field)
            except Exception:
                pass

        elif mode == 'discord':
            token_field = TextArea(height=1, multiline=False, text=self.config.dc_token)
            channel_field = TextArea(height=1, multiline=False, text=self.config.dc_chan)

            def on_ok() -> None:
                self.config.dc_token = token_field.text.strip()
                self.config.dc_chan = channel_field.text.strip()
                self._close_dialog()
                self._refresh_ui()

            # Create keybindings for the dialog body
            kb_body = KeyBindings()

            @kb_body.add('escape')
            def _escape(event):
                self._close_dialog()

            dialog = Dialog(
                title='Discord Configuration',
                body=HSplit([
                    Label(text='Bot Token:'),
                    token_field,
                    Window(height=1),
                    Label(text='Channel ID:'),
                    channel_field,
                ], key_bindings=kb_body),
                buttons=[
                    Button('Save (Enter)', handler=on_ok),
                    Button('Cancel (Esc)', handler=self._close_dialog)
                ],
                width=Dimension(min=60, max=90, preferred=75),
                modal=True
            )

            self._open_dialog(dialog, on_ok)
            try:
                self.app.layout.focus(token_field)
            except Exception:
                pass

    def _confirm_and_launch(self) -> None:
        """Validate configuration and check inbox before launching orchestrator"""
        # Clear error
        self.error_msg = ''
        self._refresh_ui()

        # Save IM configuration before validation
        self._save_im_config()

        # Validate configuration
        valid, error = self.config.is_valid(self.actors_available, self.home)
        if not valid:
            self.error_msg = error
            self._refresh_ui()
            return

        # Check foreman/aux compatibility
        if getattr(self.config, 'foreman', None) == 'reuse_aux':
            aux_val = getattr(self.config, 'aux', None)
            if not aux_val or aux_val == 'none':
                self.error_msg = "Foreman is set to 'reuse_aux' but Aux is not configured.\nPlease set Aux to an actor, or change Foreman to 'none' or a specific actor."
                self._refresh_ui()
                return

        # Check actor availability
        missing_actors = []
        for role, actor in [('PeerA', self.config.peerA), ('PeerB', self.config.peerB), ('Aux', self.config.aux)]:
            if actor and actor != 'none':
                available, hint = self.actor_availability.get(actor, (True, "Unknown"))
                if not available:
                    missing_actors.append(f"{role} ({actor}): {hint}")

        if missing_actors:
            error_lines = ["Cannot launch - required actors not installed:"] + missing_actors
            error_lines.append("\nInstall missing actors and restart setup.")
            self.error_msg = "\n".join(error_lines)
            self._refresh_ui()
            return

        # Save config FIRST - this writes foreman.yaml that footer needs
        self._save_config()
        
        # Transition to runtime UI
        self.setup_visible = False
        self._build_runtime_ui()

        # Write initial timeline message
        self._write_timeline("Configuration validated", 'system')
        self._write_timeline(f"PeerA: {self.config.peerA}", 'success')
        self._write_timeline(f"PeerB: {self.config.peerB}", 'success')
        if self.config.aux and self.config.aux != 'none':
            self._write_timeline(f"Aux: {self.config.aux}", 'success')

        # Check for residual inbox messages BEFORE launching
        # If there are messages, this will show a dialog and return
        # The dialog handlers will call _continue_launch() to proceed
        self._check_residual_inbox()

    def _continue_launch(self) -> None:
        """Continue with orchestrator launch after inbox check is complete"""
        # Config already saved in _confirm_and_launch

        self._write_timeline("Launching orchestrator...", 'system')
        self._write_timeline("Type /help for commands", 'info')

    def _quit_app(self) -> None:
        """Quit CCCC by detaching from tmux (if in tmux) or exiting app"""
        import subprocess
        import os

        # Check if we're in a tmux session
        if os.environ.get('TMUX'):
            try:
                # Detach from tmux session (this will kill all processes in the session)
                subprocess.run(['tmux', 'detach-client'], check=False)
            except Exception:
                # If tmux detach fails, just exit the app
                self.app.exit()
        else:
            # Not in tmux, just exit the app
            self.app.exit()

    def _setup_timeline_mouse_scroll(self) -> None:
        """Setup mouse wheel scrolling and text selection for timeline"""
        from prompt_toolkit.mouse_events import MouseEventType
        from prompt_toolkit.selection import SelectionState, SelectionType

        # Get the original mouse handler
        original_handler = self.timeline.window.content.mouse_handler

        # Track drag state for selection
        self._mouse_drag_start_pos = None

        def custom_mouse_handler(mouse_event):
            buffer = self.timeline.buffer

            # Handle scroll events
            if mouse_event.event_type == MouseEventType.SCROLL_DOWN:
                for _ in range(3):
                    buffer.cursor_down()
                return None
            elif mouse_event.event_type == MouseEventType.SCROLL_UP:
                for _ in range(3):
                    buffer.cursor_up()
                return None

            # Handle mouse selection (drag to select)
            elif mouse_event.event_type == MouseEventType.MOUSE_DOWN:
                # Start selection - calculate position from mouse coordinates
                # First, call original handler to move cursor
                if original_handler:
                    original_handler(mouse_event)
                # Record start position for drag selection
                self._mouse_drag_start_pos = buffer.cursor_position
                # Clear any existing selection
                buffer.selection_state = None
                return None

            elif mouse_event.event_type == MouseEventType.MOUSE_MOVE:
                # Dragging - update selection
                if self._mouse_drag_start_pos is not None:
                    # Move cursor to current mouse position
                    if original_handler:
                        original_handler(mouse_event)
                    # Create/update selection from start to current position
                    buffer.selection_state = SelectionState(
                        original_cursor_position=self._mouse_drag_start_pos,
                        type=SelectionType.CHARACTERS
                    )
                return None

            elif mouse_event.event_type == MouseEventType.MOUSE_UP:
                # End selection and auto-copy to clipboard
                if self._mouse_drag_start_pos is not None:
                    if original_handler:
                        original_handler(mouse_event)
                    # If we actually dragged (selected text)
                    if buffer.cursor_position != self._mouse_drag_start_pos:
                        buffer.selection_state = SelectionState(
                            original_cursor_position=self._mouse_drag_start_pos,
                            type=SelectionType.CHARACTERS
                        )
                        # Auto-copy selection to clipboard (macOS)
                        try:
                            start = min(self._mouse_drag_start_pos, buffer.cursor_position)
                            end = max(self._mouse_drag_start_pos, buffer.cursor_position)
                            selected_text = buffer.document.text[start:end]
                            if selected_text.strip():
                                subprocess.run(['pbcopy'], input=selected_text, text=True, timeout=2)
                                self._write_timeline(f"Copied {len(selected_text.splitlines())} lines (Cmd+V to paste)", 'success')
                        except Exception:
                            pass
                    self._mouse_drag_start_pos = None
                return None

            # For other events, use original handler
            if original_handler:
                return original_handler(mouse_event)
            return None

        # Replace the mouse handler
        self.timeline.window.content.mouse_handler = custom_mouse_handler

    def _build_runtime_ui(self) -> None:
        """Build modern runtime UI (Full-width Timeline + Input + Footer)"""
        # Wrap input field with clear Frame border and title
        input_with_frame = Frame(
            body=self.input_field,
            title='Message (Enter to send, Esc to focus timeline)',
            style='class:input-frame'
        )
        
        # Task text function for header (shows progress inline)
        def get_task_header_text():
            if self.task_panel:
                return self.task_panel.get_header_text()
            return ""
        
        # Task click handler for mouse support
        def on_task_click():
            self._toggle_task_panel()
        
        # Rebuild root with clean, full-width layout including task panel
        self.root.content = HSplit([
            create_runtime_header(task_text_func=get_task_header_text, on_task_click=on_task_click),
            self._build_task_panel_container(),  # WBS view when T pressed
            Window(height=1),
            # Timeline label
            Window(
                content=FormattedTextControl([('class:section', '💬 Conversation:')]),
                height=1,
                dont_extend_height=True
            ),
            # Timeline - simple and direct
            self.timeline,
            Window(height=1),
            input_with_frame,
            Window(
                content=FormattedTextControl(self._get_footer_text),
                height=Dimension(min=5, max=5),  # Reduced from 6, task status now in header
                dont_extend_height=True
            ),
        ])

        # Focus timeline by default (for natural mouse scrolling)
        try:
            self.app.layout.focus(self.timeline)
        except Exception:
            pass

    def _save_config(self) -> None:
        """Save configuration directly to yaml files and trigger orchestrator launch"""
        ts = time.time()

        # 1. Save roles and mode to cli_profiles.yaml
        # Load existing config to preserve delivery, tmux, and other settings
        cli_profiles = _load_yaml(self.home, 'settings/cli_profiles.yaml')

        # Update roles - preserve existing configuration, only update actor and cwd
        roles = cli_profiles.get('roles') or {}

        # Handle PeerA - always required
        if self.config.peerA and self.config.peerA != 'none':
            if 'peerA' not in roles:
                roles['peerA'] = {}
            roles['peerA']['actor'] = self.config.peerA
            roles['peerA']['cwd'] = '.'

        # Handle PeerB - 'none' means single-peer mode
        # IMPORTANT: Always write peerB section explicitly to preserve user's choice
        # Writing 'actor: none' ensures single-peer mode persists across restarts
        if 'peerB' not in roles:
            roles['peerB'] = {}
        if self.config.peerB and self.config.peerB != 'none':
            roles['peerB']['actor'] = self.config.peerB
        else:
            # Single-peer mode: write explicit 'none' instead of deleting section
            roles['peerB']['actor'] = 'none'
        roles['peerB']['cwd'] = '.'

        # Handle aux separately - always write or clear to ensure changes take effect
        if self.config.aux and self.config.aux != 'none':
            if 'aux' not in roles:
                roles['aux'] = {}
            roles['aux']['actor'] = self.config.aux
            roles['aux']['cwd'] = '.'
        else:
            # Aux disabled: remove aux section entirely to prevent stale config
            if 'aux' in roles:
                del roles['aux']

        cli_profiles['roles'] = roles

        # Update mode selection
        cli_profiles['im_mode'] = self.config.mode

        # Write back, preserving all other fields (delivery, tmux, etc.)
        _write_yaml(self.home, 'settings/cli_profiles.yaml', cli_profiles)

        # 2. Save foreman config with all settings exposed (preserving user customizations)
        foreman_config = _load_yaml(self.home, 'settings/foreman.yaml')
        # Set defaults for all configurable fields so users can see and modify them
        foreman_config.setdefault('interval_seconds', 900)      # 15 minutes between runs
        foreman_config.setdefault('max_run_seconds', 900)       # Max duration per run
        foreman_config.setdefault('prompt_path', './FOREMAN_TASK.md')
        foreman_config.setdefault('cc_user', True)              # Copy output to user
        
        # Self-optimization config (preserve existing or create from TUI toggle)
        if 'self_optimization' not in foreman_config:
            foreman_config['self_optimization'] = {}
        # Update enabled flag from TUI toggle
        foreman_config['self_optimization']['enabled'] = self.config.self_opt_enabled
        # Set defaults for other fields if not present
        foreman_config['self_optimization'].setdefault('run_mode', 'before_task')
        foreman_config['self_optimization'].setdefault('quick_mode', False)
        foreman_config['self_optimization'].setdefault('min_interval_hours', 0.5)
        
        if self.config.foreman and self.config.foreman != 'none':
            foreman_config['agent'] = self.config.foreman
            foreman_config['enabled'] = True
            foreman_config['allowed'] = True
        else:
            # Foreman disabled: clear agent to ensure _is_foreman_configured returns False
            foreman_config['agent'] = 'none'
            foreman_config['enabled'] = False
            foreman_config['allowed'] = False
        
        _write_yaml(self.home, 'settings/foreman.yaml', foreman_config)

        # 3. Save IM provider configuration directly to yaml files
        # Load existing configs to preserve advanced settings (routing, files, outbound, etc.)

        if self.config.mode == 'telegram' and self.config.tg_token:
            telegram_config = _load_yaml(self.home, 'settings/telegram.yaml')
            telegram_config['token'] = self.config.tg_token
            telegram_config['token_env'] = 'TELEGRAM_BOT_TOKEN'
            telegram_config['autostart'] = True
            if self.config.tg_chat:
                telegram_config['allow_chats'] = [self.config.tg_chat]
            _write_yaml(self.home, 'settings/telegram.yaml', telegram_config)

        elif self.config.mode == 'slack' and self.config.sl_bot_token:
            slack_config = _load_yaml(self.home, 'settings/slack.yaml')
            # Save bot token
            slack_config['bot_token'] = self.config.sl_bot_token
            slack_config['bot_token_env'] = 'SLACK_BOT_TOKEN'
            # Save app token if provided
            if self.config.sl_app_token:
                slack_config['app_token'] = self.config.sl_app_token
                slack_config['app_token_env'] = 'SLACK_APP_TOKEN'
            # Set autostart
            slack_config['autostart'] = True
            # Save channel configuration
            if self.config.sl_chan:
                channels = slack_config.get('channels') or {}
                channels['to_user'] = [self.config.sl_chan]
                channels['to_peer_summary'] = [self.config.sl_chan]
                slack_config['channels'] = channels
            _write_yaml(self.home, 'settings/slack.yaml', slack_config)

        elif self.config.mode == 'discord' and self.config.dc_token:
            discord_config = _load_yaml(self.home, 'settings/discord.yaml')
            discord_config['bot_token'] = self.config.dc_token
            discord_config['bot_token_env'] = 'DISCORD_BOT_TOKEN'
            discord_config['autostart'] = True
            if self.config.dc_chan:
                try:
                    ch_id = int(self.config.dc_chan)
                except (ValueError, TypeError):
                    ch_id = self.config.dc_chan
                channels = discord_config.get('channels') or {}
                channels['to_user'] = [ch_id]
                channels['to_peer_summary'] = [ch_id]
                discord_config['channels'] = channels
            _write_yaml(self.home, 'settings/discord.yaml', discord_config)

        # WeCom: outbound-only webhook
        elif self.config.mode == 'wecom' and self.config.wc_webhook:
            wecom_config = _load_yaml(self.home, 'settings/wecom.yaml')
            wecom_config['webhook_url'] = self.config.wc_webhook
            wecom_config['autostart'] = True
            wecom_config['message_type'] = 'markdown_v2'
            _write_yaml(self.home, 'settings/wecom.yaml', wecom_config)

        # 4. Write launch command to trigger orchestrator startup
        # IMPORTANT: Use 'w' mode to overwrite, not 'a' to append
        # Each setup completion should start fresh, not mix with previous sessions
        cmds = self.home / "state" / "commands.jsonl"
        cmds.parent.mkdir(parents=True, exist_ok=True)
        with cmds.open('w', encoding='utf-8') as f:
            cmd = {"type": "launch", "args": {"who": "both"}, "source": "tui", "ts": ts}
            f.write(json.dumps(cmd, ensure_ascii=False) + '\n')

        # Confirmation flag
        (self.home / "state" / "settings.confirmed").write_text(str(int(ts)))

    def _get_dynamic_prompt(self) -> str:
        """Dynamic prompt that shows current mode and state"""
        if self.reverse_search_mode:
            # Search mode: show query
            if self.search_results:
                count = f"{self.search_index + 1}/{len(self.search_results)}"
                return f"(search: '{self.search_query}' {count}) ❯ "
            else:
                return f"(search: '{self.search_query}' no matches) ❯ "
        else:
            # Normal mode: clean prompt
            return '❯ '

    def _write_timeline(self, text: str, msg_type: str = 'info', silent: bool = False) -> None:
        """
        Append message with two-line header format for clean alignment.

        Format:
            HH:MM 🤖 SENDER
            │ Message content
            │ Continuation lines

        Args:
            text: Message content
            msg_type: system/peerA/peerB/user/error/info/success/warning/debug
            silent: If True, don't increment message count
        """
        timestamp = time.strftime('%H:%M')

        # Sender config: (icon, label) - no colors, pure text
        # NOTE: All icons should be East Asian Width = W (Wide) to avoid
        # prompt_toolkit width miscalculation. VS16 emoji (⚙️, ℹ️, ⚠️) are problematic.
        sender_config = {
            'system': ('🔧', 'SYS'),
            'peerA': ('🤖', self.config.peerA if hasattr(self.config, 'peerA') else 'PeerA'),
            'peerB': ('🔩', self.config.peerB if hasattr(self.config, 'peerB') else 'PeerB'),
            'user': ('👤', 'YOU'),
            'error': ('❌', 'ERR'),
            'info': ('💡', 'INF'),
            'success': ('✅', 'OK'),
            'warning': ('🚨', 'WRN'),
            'debug': ('🔍', 'DBG'),
        }

        icon, label = sender_config.get(msg_type, ('•', msg_type.upper()[:3]))

        lines = []

        # Dynamic wrap width based on terminal size
        # Reserve some margin for scrollbar (2) and line prefix "│ " (2) and safety margin (4)
        try:
            term_width = shutil.get_terminal_size(fallback=(120, 40)).columns
            max_width = max(40, term_width - 8)  # Minimum 40, subtract margins
        except Exception:
            max_width = 100  # Fallback to reasonable default

        text_lines = []
        for line in text.split('\n'):
            if not line:
                text_lines.append('')
                continue
            while len(line) > max_width:
                split_point = line.rfind(' ', 0, max_width)
                if split_point == -1:
                    split_point = max_width
                text_lines.append(line[:split_point])
                line = line[split_point:].lstrip()
            text_lines.append(line)

        # Format message - two-line header format
        # Line 1: header (timestamp + icon + label)
        # Line 2+: content (│ at column 0 for perfect alignment)
        if self.timeline.text:
            lines.append('')  # Blank line between messages
        lines.append(f'{timestamp} {icon} {label}')  # Header line
        for text_line in text_lines:
            lines.append(f'│ {text_line}')  # Content lines, │ at column 0

        # Append to timeline
        formatted = '\n'.join(lines) + '\n'
        current = self.timeline.text
        self.timeline.text = current + formatted

        # Update message count (unless silent)
        if not silent:
            self.message_count += 1

        # Auto-scroll to bottom
        self.timeline.buffer.cursor_position = len(self.timeline.text)

    def _start_reverse_search(self) -> None:
        """Start reverse search mode"""
        self.reverse_search_mode = True
        self.search_query = ''
        self.search_results = []
        self.search_index = 0
        self._update_search_prompt()

    def _update_search_prompt(self) -> None:
        """Update input prompt for reverse search"""
        if self.reverse_search_mode:
            if self.search_results:
                result = self.search_results[self.search_index]
                self.input_field.text = result
                # Show search status at bottom of timeline
                status = f"(reverse-i-search)`{self.search_query}': {result}"
                # Update a temporary status line (we'll show it in the prompt)
            else:
                self.input_field.text = ''

    def _perform_reverse_search(self, query: str) -> None:
        """Perform reverse search through command history"""
        self.search_query = query
        self.search_results = []
        self.search_index = 0

        if not query:
            self._update_search_prompt()
            return

        # Search through history in reverse order
        query_lower = query.lower()
        for cmd in reversed(self.command_history):
            if query_lower in cmd.lower():
                self.search_results.append(cmd)

        self._update_search_prompt()

    def _exit_reverse_search(self, accept: bool = False) -> None:
        """Exit reverse search mode"""
        if accept and self.search_results and self.search_index < len(self.search_results):
            # Keep the selected command in input
            self.input_field.text = self.search_results[self.search_index]
        else:
            # Restore original input or clear
            self.input_field.text = ''

        self.reverse_search_mode = False
        self.search_query = ''
        self.search_results = []
        self.search_index = 0

    def _update_status(self) -> None:
        """Update connection state (status panel removed, info now in footer)"""
        try:
            status_path = self.home / "state" / "status.json"
            if status_path.exists():
                # Just update connection state
                self.orchestrator_connected = True
                self.last_update_time = time.time()
            else:
                self.orchestrator_connected = False
        except Exception:
            pass

    def _get_footer_text(self) -> list:
        """
        Generate dynamic footer text with 3-row layout.
        Called by FormattedTextControl on each render.

        Row 1: Agent configuration (peerA, peerB, aux, foreman)
        Row 2: File checks (PROJECT.md, FOREMAN_TASK.md) + Connection mode
        Row 3: Mailbox stats + Active handoffs + Last activity time
        """
        # Read status.json for runtime state
        status_data = {}
        try:
            status_path = self.home / "state" / "status.json"
            if status_path.exists():
                status_data = json.loads(status_path.read_text(encoding='utf-8'))
        except Exception:
            pass

        # Read bridge warnings for missing optional dependencies (slack/discord)
        warnings = []
        try:
            warn_path = self.home / "state" / "bridge-warnings.json"
            if warn_path.exists():
                w = json.loads(warn_path.read_text(encoding='utf-8')) or {}
                # Show warnings only when corresponding provider is configured to run
                # (avoid stale warnings from older sessions)
                # Telegram rarely has dependency issues, but keep structure uniform.
                # Slack: require autostart and bot_token in settings/slack.yaml
                # Discord: require autostart and bot_token in settings/discord.yaml
                def _provider_enabled(name: str) -> bool:
                    try:
                        import yaml  # local import
                        cfgp = self.home / "settings" / f"{name}.yaml"
                        if not cfgp.exists():
                            return False
                        cfg = yaml.safe_load(cfgp.read_text(encoding='utf-8')) or {}
                        if not bool(cfg.get('autostart', name == 'telegram')):
                            return False
                        if name == 'telegram':
                            return bool(cfg.get('token'))
                        if name == 'slack':
                            return bool(cfg.get('bot_token'))
                        if name == 'discord':
                            return bool(cfg.get('bot_token'))
                    except Exception:
                        return False
                    return False
                for adapter in ("telegram", "slack", "discord"):
                    ent = w.get(adapter)
                    if not ent:
                        continue
                    if not _provider_enabled(adapter):
                        continue
                    msg = str(ent.get("message") or "")
                    if msg:
                        short = msg if len(msg) <= 120 else (msg[:117] + "…")
                        warnings.append(f"{adapter}: {short}")
        except Exception:
            pass

        # Read ledger.jsonl for activity stats (last 100 entries)
        ledger_items = []
        try:
            ledger_path = self.home / "state" / "ledger.jsonl"
            if ledger_path.exists():
                with ledger_path.open("r", encoding="utf-8") as f:
                    lines = f.readlines()
                    # Only read last 100 lines for performance
                    for line in lines[-100:]:
                        line = line.strip()
                        if line:
                            try:
                                ledger_items.append(json.loads(line))
                            except Exception:
                                pass
        except Exception:
            pass

        # === Row 1: Agent Configuration ===
        peerA = self.config.peerA or 'none'
        peerB = self.config.peerB or 'none'

        # Aux status
        aux_agent = self.config.aux or 'none'
        aux_enabled = status_data.get('aux', {}).get('mode', 'off') != 'off' if isinstance(status_data.get('aux'), dict) else False
        aux_status = 'ON' if aux_enabled else 'OFF'
        aux_display = f"{aux_agent} ({aux_status})" if aux_agent != 'none' else 'none'

        # Foreman status
        foreman_agent = self.config.foreman or 'none'
        foreman_data = status_data.get('foreman', {})
        
        # If status.json doesn't have foreman data, fallback to reading foreman.yaml directly
        if not foreman_data or not isinstance(foreman_data, dict):
            try:
                import yaml
                fc_path = self.home / "settings" / "foreman.yaml"
                if fc_path.exists():
                    fc = yaml.safe_load(fc_path.read_text(encoding='utf-8')) or {}
                    foreman_enabled = bool(fc.get('enabled', False))
                else:
                    foreman_enabled = False
            except Exception:
                foreman_enabled = False
        else:
            foreman_enabled = foreman_data.get('enabled', False)
        
        foreman_status = 'ON' if foreman_enabled else 'OFF'
        foreman_display = f"{foreman_agent} ({foreman_status})" if foreman_agent != 'none' else 'none'

        # Format peer display - single-peer mode shows differently
        if peerB == 'none' or peerB.lower() == 'none':
            peer_display = f"{peerA} (single)"
        else:
            peer_display = f"{peerA} ⇄ {peerB}"
        
        row1 = f"Agents: {peer_display} │ Aux: {aux_display} │ Foreman: {foreman_display}"

        # === Row 2: File Checks + Connection Mode ===
        # Check PROJECT.md (in repo root, which is home.parent)
        repo_root = self.home.parent
        project_md_exists = (repo_root / "PROJECT.md").exists()
        project_md_icon = '✓' if project_md_exists else '✗'

        # Check FOREMAN_TASK.md (only if foreman is configured)
        foreman_task_md_icon = ''
        if foreman_agent != 'none':
            foreman_task_exists = (repo_root / "FOREMAN_TASK.md").exists()
            foreman_task_md_icon = f" FOREMAN_TASK.md{('✓' if foreman_task_exists else '✗')}"

        # Connection mode (tmux is always on; check telegram/slack/discord from status or config)
        # Check each IM provider
        telegram_enabled = self.config.mode == 'telegram' and bool(self.config.tg_token)
        telegram_icon = '●' if telegram_enabled else '○'

        slack_enabled = self.config.mode == 'slack' and bool(self.config.sl_bot_token)
        slack_icon = '●' if slack_enabled else '○'

        discord_enabled = self.config.mode == 'discord' and bool(self.config.dc_token)
        discord_icon = '●' if discord_enabled else '○'

        # Build connection mode string
        mode_parts = ['tmux●']  # tmux is always active

        # Add configured IM providers
        if telegram_enabled:
            mode_parts.append('telegram●')
        if slack_enabled:
            mode_parts.append('slack●')
        if discord_enabled:
            mode_parts.append('discord●')

        # Show all configured modes, or just tmux if no IM modes are active
        if len(mode_parts) > 1:
            mode_str = '+'.join(mode_parts)
        else:
            mode_str = 'tmux'

        row2 = f"Files: PROJECT.md{project_md_icon}{foreman_task_md_icon} │ Mode: {mode_str}"

        # === Row 3: Mailbox Stats + Activity ===
        # Direct count of inbox and processed files
        def count_files(peer: str, subdir: str) -> int:
            """Count files in mailbox subdirectory"""
            path = self.home / "mailbox" / peer / subdir
            try:
                return len([f for f in path.iterdir() if f.is_file()]) if path.exists() else 0
            except Exception:
                return 0

        a_inbox = count_files('peerA', 'inbox')
        a_processed = count_files('peerA', 'processed')
        b_inbox = count_files('peerB', 'inbox')
        b_processed = count_files('peerB', 'processed')

        # Format: A(inbox/processed) B(inbox/processed)
        mailbox_str = f"A({a_inbox}/{a_processed}) B({b_inbox}/{b_processed})"

        # Count active handoffs from ledger (handoffs with status=queued or delivered in last 10 items)
        active_handoffs = 0
        for item in ledger_items[-10:]:
            if item.get('kind') == 'handoff' and item.get('status') in ['queued', 'delivered']:
                active_handoffs += 1

        # Last activity time (from most recent ledger entry with timestamp)
        last_activity_str = '-'
        if ledger_items:
            last_item = ledger_items[-1]
            ts_str = last_item.get('ts', '')
            if ts_str:
                try:
                    # Parse timestamp like "12:34:56" and compute elapsed time
                    # For simplicity, we'll just show "-" as we don't have full timestamp
                    # In production, ledger should include Unix timestamp
                    last_activity_str = 'just now'
                except Exception:
                    last_activity_str = '-'

        row3 = f"Mailbox: {mailbox_str} │ Active: {active_handoffs} handoffs │ Last: {last_activity_str}"

        # Note: Task status moved to header (accessible via T key for expanded WBS view)

        # Check if handoff is paused
        is_paused = status_data.get('paused', False)

        # === Build formatted text with proper styling ===
        text = []
        # Optional row 0: warnings banner when present
        if warnings:
            warn_line = '  • '.join(warnings)
            text.extend([
                ('class:warning', '⚠ Dependencies: '),
                ('class:warning', warn_line),
                ('', '\n')
            ])
        # PAUSED status banner (prominent display when handoff is paused)
        if is_paused:
            text.extend([
                ('class:warning', '⏸ HANDOFF PAUSED'),
                ('class:info', ' - messages saved to inbox but not delivered. Use '),
                ('class:success', '/resume'),
                ('class:info', ' to continue.'),
                ('', '\n')
            ])
        text.extend([
            ('class:section', '─' * 80 + '\n'),
            ('class:info', row1), ('', '\n'),
            ('class:info', row2), ('', '\n'),
            ('class:info', row3), ('', '\n'),
            ('class:section', '─' * 80),
        ])

        return text

    def _process_command(self, text: str) -> None:
        """Process user command - routes to orchestrator via commands.jsonl"""
        text = text.strip()
        if not text:
            return

        cmds = self.home / "state" / "commands.jsonl"
        cmds.parent.mkdir(parents=True, exist_ok=True)
        ts = time.time()

        # Parse command
        if text == '/help' or text == 'h':
            # Show real orchestrator commands
            self._write_timeline("", 'info')
            self._write_timeline("=== CCCC Commands ===", 'info')
            self._write_timeline("", 'info')
            self._write_timeline("Messages:", 'info')
            self._write_timeline("  /a <text>           Send message to PeerA", 'info')
            self._write_timeline("  /b <text>           Send message to PeerB", 'info')
            self._write_timeline("  /both <text>        Send message to both peers", 'info')
            self._write_timeline("", 'info')
            self._write_timeline("Control:", 'info')
            self._write_timeline("  /pause              Pause handoff", 'info')
            self._write_timeline("  /resume             Resume handoff", 'info')
            self._write_timeline("  /restart            Restart peer CLI (peera|peerb|both)", 'info')
            self._write_timeline("  /quit               Quit CCCC (exit all processes)", 'info')
            self._write_timeline("", 'info')
            self._write_timeline("Operations:", 'info')
            self._write_timeline("  /foreman on|off|status|now   Control background scheduler", 'info')
            self._write_timeline("  /aux <prompt>       Run Aux helper", 'info')
            self._write_timeline("  /verbose on|off     Toggle peer summaries", 'info')
            self._write_timeline("  /task               Show task status", 'info')
            self._write_timeline("  /paste              Paste image from clipboard", 'info')
            self._write_timeline("", 'info')
            self._write_timeline("Keyboard:", 'info')
            self._write_timeline("  Ctrl+T              Focus timeline (enable mouse scroll)", 'info')
            self._write_timeline("  T                   Toggle task panel", 'info')
            self._write_timeline("    ↑↓/jk             Select task (when panel open)", 'info')
            self._write_timeline("    Enter             Show task detail", 'info')
            self._write_timeline("    Esc               Close task panel", 'info')
            self._write_timeline("  Esc                 Return to input from timeline", 'info')
            self._write_timeline("  Ctrl+A/E            Start/end of line", 'info')
            self._write_timeline("  Ctrl+W/U/K          Delete word/start/end", 'info')
            self._write_timeline("  Ctrl+V              Paste (image or text)", 'info')
            self._write_timeline("  Up/Down             History", 'info')
            self._write_timeline("  PageUp/Down         Scroll timeline", 'info')
            self._write_timeline("  Ctrl+L              Clear timeline", 'info')
            self._write_timeline("", 'info')
            self._write_timeline("Text Selection:", 'info')
            self._write_timeline("  Mouse drag          Select text (auto-copy)", 'info')
            self._write_timeline("  Shift+Mouse         Select text (bypass TUI mouse)", 'info')
            self._write_timeline("", 'info')
            self._write_timeline("Exit:", 'info')
            self._write_timeline("  Ctrl+b d            Detach tmux (exits CCCC)", 'info')
            self._write_timeline("", 'info')
            self._write_timeline("==================", 'info')

        # Simple control commands (no arguments)
        elif text == '/pause':
            self._write_cmd_to_queue("pause", {}, "Pause command sent")
        elif text == '/resume':
            self._write_cmd_to_queue("resume", {}, "Resume command sent")
        elif text.startswith('/restart '):
            target = text[9:].strip().lower()
            if target in ('peera', 'peerb', 'both', 'a', 'b'):
                self._write_cmd_to_queue("restart", {"target": target}, f"Restart {target} command sent")
            else:
                self._write_timeline("Usage: /restart peera|peerb|both", 'error')
        elif text == '/quit' or text == 'q':
            self._write_timeline("Shutting down CCCC...", 'system')
            self._quit_app()

        # Foreman commands
        elif text.startswith('/foreman '):
            arg = text[9:].strip()
            if arg in ('on', 'off', 'status', 'now'):
                self._write_cmd_to_queue("foreman", {"action": arg}, f"Foreman {arg} command sent")
            else:
                self._write_timeline("Usage: /foreman on|off|status|now", 'error')

        # Verbose toggle
        elif text.startswith('/verbose '):
            arg = text[9:].strip().lower()
            if arg in ('on','off'):
                self._write_cmd_to_queue("verbose", {"value": arg}, f"Verbose {arg} sent")
            else:
                self._write_timeline("Usage: /verbose on|off", 'error')

        # Paste image from clipboard
        elif text == '/paste':
            self._handle_paste_image()

        # Blueprint task status
        elif text == '/task' or text.startswith('/task '):
            self._show_task_status(text)

        # Aux command with prompt
        elif text.startswith('/aux '):
            prompt = text[5:].strip()
            if prompt:
                self._write_cmd_to_queue("aux", {"prompt": prompt}, "Aux command sent")
            else:
                self._write_timeline("Usage: /aux <prompt>", 'error')

        # Message sending commands (with image support)
        elif text.startswith('/a '):
            msg = text[3:].strip()
            if msg:
                # Parse for image paths
                cleaned_msg, images = parse_message_images(msg)
                display_msg = msg if not images else f"{cleaned_msg} [+{len(images)} image(s)]"
                self._write_timeline(f"You > PeerA: {display_msg}", 'user')
                try:
                    with cmds.open('a', encoding='utf-8') as f:
                        args = {"text": cleaned_msg if cleaned_msg else "Please see the attached image(s)."}
                        if images:
                            args["images"] = images
                        cmd = {"type": "a", "args": args, "source": "tui", "ts": ts}
                        f.write(json.dumps(cmd, ensure_ascii=False) + '\n')
                        f.flush()
                    self._write_timeline("Sent to PeerA", 'success')
                except Exception as e:
                    self._write_timeline(f"Failed to send: {str(e)[:50]}", 'error')
            else:
                self._write_timeline("Usage: /a <message>", 'error')

        elif text.startswith('/b '):
            # Check single-peer mode before sending
            try:
                from common.config import is_single_peer_mode
                if is_single_peer_mode(self.home):
                    self._write_timeline("Single-peer mode: use /a instead of /b", 'error')
                    return
            except Exception:
                pass  # Continue with normal flow if check fails
            msg = text[3:].strip()
            if msg:
                # Parse for image paths
                cleaned_msg, images = parse_message_images(msg)
                display_msg = msg if not images else f"{cleaned_msg} [+{len(images)} image(s)]"
                self._write_timeline(f"You > PeerB: {display_msg}", 'user')
                try:
                    with cmds.open('a', encoding='utf-8') as f:
                        args = {"text": cleaned_msg if cleaned_msg else "Please see the attached image(s)."}
                        if images:
                            args["images"] = images
                        cmd = {"type": "b", "args": args, "source": "tui", "ts": ts}
                        f.write(json.dumps(cmd, ensure_ascii=False) + '\n')
                        f.flush()
                    self._write_timeline("Sent to PeerB", 'success')
                except Exception as e:
                    self._write_timeline(f"Failed to send: {str(e)[:50]}", 'error')
            else:
                self._write_timeline("Usage: /b <message>", 'error')

        elif text.startswith('/both '):
            msg = text[6:].strip()
            if msg:
                # Parse for image paths
                cleaned_msg, images = parse_message_images(msg)
                display_msg = msg if not images else f"{cleaned_msg} [+{len(images)} image(s)]"
                self._write_timeline(f"You > Both: {display_msg}", 'user')
                try:
                    with cmds.open('a', encoding='utf-8') as f:
                        args = {"text": cleaned_msg if cleaned_msg else "Please see the attached image(s)."}
                        if images:
                            args["images"] = images
                        cmd = {"type": "both", "args": args, "source": "tui", "ts": ts}
                        f.write(json.dumps(cmd, ensure_ascii=False) + '\n')
                        f.flush()
                    self._write_timeline("Sent to both peers", 'success')
                except Exception as e:
                    self._write_timeline(f"Failed to send: {str(e)[:50]}", 'error')
            else:
                self._write_timeline("Usage: /both <message>", 'error')

        else:
            self._write_timeline(f"Unknown command: {text}. Type /help for help.", 'error')

    def _handle_paste_image(self) -> None:
        """Handle /paste command - get image from clipboard and insert path"""
        import tempfile
        temp_dir = Path(tempfile.gettempdir()) / "cccc_clipboard_temp"
        
        self._write_timeline("Checking clipboard for image...", 'info')
        
        try:
            image_path = get_clipboard_image(temp_dir)
        except Exception as e:
            self._write_timeline(f"Clipboard error: {e}", 'error')
            return
        
        if image_path:
            # Save to unified directory with metadata
            meta = save_image_for_cli(self.home, image_path)
            if meta:
                # Insert path reference into input buffer
                buffer = self.input_field.buffer
                buffer.insert_text(f"@{meta['path']} ")
                self._write_timeline(f"Image ready: {meta['path']} ({meta['bytes']} bytes)", 'success')
                # Clean up temp file
                try:
                    Path(image_path).unlink()
                except Exception:
                    pass
            else:
                self._write_timeline("Failed to save image to project", 'error')
        else:
            self._write_timeline("No image found in clipboard", 'info')

    def _write_cmd_to_queue(self, cmd_type: str, args: dict, success_msg: str) -> None:
        """Write command to commands.jsonl queue"""
        try:
            cmds = self.home / "state" / "commands.jsonl"
            with cmds.open('a', encoding='utf-8') as f:
                cmd = {"type": cmd_type, "args": args, "source": "tui", "ts": time.time()}
                f.write(json.dumps(cmd, ensure_ascii=False) + '\n')
                f.flush()
            self._write_timeline(success_msg, 'success')
        except Exception as e:
            self._write_timeline(f"Failed to send command: {str(e)[:50]}", 'error')

    def _show_task_status(self, text: str) -> None:
        """Show task status or task detail.
        
        Usage:
            /task          - Show all tasks summary
            /task T001     - Show detailed view of T001
            /task 1        - Show detailed view of first task
        """
        try:
            # Parse command argument
            parts = text.strip().split(maxsplit=1)
            task_arg = parts[1].strip().upper() if len(parts) > 1 else None
            
            # Use task_panel if available (it has get_task_detail)
            if self.task_panel:
                # Show specific task detail as floating dialog
                if task_arg:
                    # Support both "T001" and "1" (shorthand)
                    if task_arg.isdigit():
                        task_id = f"T{int(task_arg):03d}"
                    else:
                        task_id = task_arg
                    
                    # Show as floating dialog instead of timeline output
                    self._show_task_detail_dialog(task_id)
                    return
                
                # Show summary (no argument)
                summary = self.task_panel._get_summary()
                if summary['total_tasks'] == 0:
                    self._write_timeline("", 'info')
                    self._write_timeline("No tasks defined.", 'info')
                    self._write_timeline("Agent creates tasks in docs/por/T001-name/task.yaml", 'info')
                    return
                
                self._write_timeline("", 'info')
                self._write_timeline("=== Blueprint Tasks ===", 'info')
                self._write_timeline(f"Progress: {summary['completed_tasks']}/{summary['total_tasks']} tasks │ {summary['completed_steps']}/{summary['total_steps']} steps ({summary['progress_percent']}%)", 'info')
                if summary['current_task']:
                    self._write_timeline(f"Current: {summary['current_task']} [{summary['current_step'] or '-'}]", 'info')
                self._write_timeline("", 'info')
                
                # Task list with index for quick access
                self._write_timeline("Tasks (use /task <ID> for details):", 'info')
                for i, t in enumerate(summary['tasks'], 1):
                    if t['status'] == 'complete':
                        icon = '✓'
                    elif t['status'] == 'active':
                        icon = '→'
                    else:
                        icon = '○'
                    self._write_timeline(f"  {icon} [{i}] {t['id']} {t['name'][:22]:<22} {t['progress']}", 'info')
                
                self._write_timeline("", 'info')
                self._write_timeline("Tips: T=toggle panel │ /task T001 or /task 1=detail", 'info')
                return
            
            # Fallback: direct TaskManager access
            from orchestrator.task_manager import TaskManager
            root = self.home.parent if self.home.name == '.cccc' else self.home
            manager = TaskManager(root)
            summary = manager.get_summary()

            if summary['total_tasks'] == 0:
                self._write_timeline("No tasks defined.", 'info')
                return

            self._write_timeline("", 'info')
            self._write_timeline("=== Task Status ===", 'info')
            for t in summary['tasks']:
                icon = '✓' if t['status'] == 'complete' else '→' if t['status'] == 'active' else '○'
                self._write_timeline(f"  {icon} {t['id']} {t['name'][:25]:<25} {t['progress']}", 'info')

        except ImportError:
            self._write_timeline("Task module not available", 'error')
        except Exception as e:
            self._write_timeline(f"Failed to load tasks: {str(e)[:50]}", 'error')

    def _toggle_task_panel(self) -> None:
        """Toggle task panel expanded/collapsed state"""
        if hasattr(self, 'task_panel') and self.task_panel:
            self.task_panel.toggle()
            try:
                self.app.invalidate()
            except Exception:
                pass

    def _show_task_detail_by_index(self, index: int) -> None:
        """Show task detail by 1-based index (for keyboard shortcut 1-9)"""
        if not self.task_panel:
            return
        
        try:
            summary = self.task_panel._get_summary()
            tasks = summary.get('tasks', [])
            
            if index < 1 or index > len(tasks):
                self._write_timeline(f"No task at index {index}", 'info')
                return
            
            task_id = tasks[index - 1].get('id')
            if task_id:
                # Show detail and collapse panel
                detail = self.task_panel.get_task_detail(task_id)
                self._write_timeline("", 'info')
                for line in detail.split('\n'):
                    self._write_timeline(line, 'info')
                self._write_timeline("", 'info')
                
                # Collapse panel after showing detail
                if self.task_panel.expanded:
                    self.task_panel.toggle()
                    try:
                        self.app.invalidate()
                    except Exception:
                        pass
        except Exception as e:
            self._write_timeline(f"Failed to show task: {str(e)[:50]}", 'error')

    def _show_task_detail_dialog(self, task_id: str) -> None:
        """Show task detail as a floating dialog (Level 2) over task list (Level 1).
        
        Supports navigation:
        - Keyboard: ← → to navigate, Esc to close
        - Mouse: Click Prev/Next/Close buttons
        """
        if not self.task_panel:
            return
        
        # Store current task ID for navigation
        self._current_detail_task_id = task_id
        
        # Get terminal size for adaptive sizing
        try:
            term_width = self.app.output.get_size().columns if self.app else 100
            term_height = self.app.output.get_size().rows if self.app else 30
        except Exception:
            term_width, term_height = 100, 30
        
        # Calculate dialog dimensions (80% of terminal, with reasonable min/max)
        dialog_width = max(75, min(130, int(term_width * 0.85)))
        dialog_height = max(18, min(38, int(term_height * 0.75)))
        
        # Get task list for navigation
        summary = self.task_panel._get_summary()
        task_ids = [t['id'] for t in summary.get('tasks', [])]
        current_idx = task_ids.index(task_id) if task_id in task_ids else 0
        total_tasks = len(task_ids)
        
        # Get task detail with proper width for content
        detail_text = self.task_panel.get_task_detail_plain(task_id, width=dialog_width - 8)
        
        def on_close():
            self._close_task_detail_dialog()
        
        def on_prev():
            """Navigate to previous task"""
            if total_tasks > 1 and current_idx > 0:
                prev_id = task_ids[current_idx - 1]
                self._close_dialog()
                self._show_task_detail_dialog(prev_id)
        
        def on_next():
            """Navigate to next task"""
            if total_tasks > 1 and current_idx < total_tasks - 1:
                next_id = task_ids[current_idx + 1]
                self._close_dialog()
                self._show_task_detail_dialog(next_id)
        
        # Create content area with scrolling
        content_window = Window(
            content=FormattedTextControl(detail_text),
            style='class:task-detail',
            wrap_lines=False,
        )
        
        scrollable_content = ScrollablePane(
            content=content_window,
            show_scrollbar=True,
        )
        
        # Position indicator in title
        if total_tasks > 0:
            pos_indicator = f" ({current_idx + 1}/{total_tasks})"
        else:
            pos_indicator = ""
        
        # Create fixed-position button bar: [◀ Prev] (left) | [Close (Esc)] (center) | [Next ▶] (right)
        # Buttons always in fixed positions; show placeholder when not applicable
        can_prev = total_tasks > 1 and current_idx > 0
        can_next = total_tasks > 1 and current_idx < total_tasks - 1
        
        # Create buttons (or placeholders for consistent layout)
        if can_prev:
            prev_btn = Button(text="◀ Prev", handler=on_prev, width=12)
        else:
            # Invisible placeholder - same width as button
            prev_btn = Window(width=12, height=1)
        
        close_btn = Button(text="Close (Esc)", handler=on_close, width=14)
        
        if can_next:
            next_btn = Button(text="Next ▶", handler=on_next, width=12)
        else:
            # Invisible placeholder - same width as button
            next_btn = Window(width=12, height=1)
        
        # Fixed three-column button bar layout
        button_bar = VSplit([
            prev_btn,                                    # Left: Prev
            Window(width=Dimension(weight=1)),           # Flexible spacer
            close_btn,                                   # Center: Close
            Window(width=Dimension(weight=1)),           # Flexible spacer  
            next_btn,                                    # Right: Next
        ], height=1, padding=1)
        
        # Create dialog with custom button bar in body (not using Dialog's buttons param)
        dialog = Dialog(
            title=f"📋 Task {task_id}{pos_indicator}",
            body=HSplit([
                Frame(
                    body=scrollable_content,
                    style='class:task-detail',
                ),
                Window(height=1),  # Spacer before buttons
                button_bar,        # Custom fixed-position button bar
            ], width=Dimension(min=75, max=dialog_width, preferred=dialog_width),
               height=Dimension(min=15, max=dialog_height, preferred=dialog_height)),
            buttons=[],  # No default buttons - using custom layout above
            with_background=True,
        )
        
        # Mark task detail as open (Level 2)
        self.task_detail_open = True
        self._current_task_ids = task_ids
        self._current_task_idx = current_idx
        self._open_dialog(dialog)
    
    def _close_task_detail_dialog(self) -> None:
        """Close task detail dialog (Level 2) and return to task list (Level 1)."""
        self.task_detail_open = False
        self._close_dialog()
        # Focus stays on input, task panel (Level 1) remains visible
        try:
            self.app.invalidate()
        except Exception:
            pass

    def _open_dialog(self, dialog: Dialog, ok_handler: Optional[callable] = None, key_bindings: Optional[KeyBindings] = None) -> None:
        """Open dialog with optional key bindings."""
        if self.modal_open:
            return

        float_dialog = Float(content=dialog)
        # Update root container to include the dialog
        if hasattr(self.root, 'floats'):
            self.root.floats.append(float_dialog)
        else:
            # Fallback: store in local floats list
            self.floats.append(float_dialog)
        
        # Store key bindings for dialog if provided
        if key_bindings:
            self._dialog_key_bindings = key_bindings
        else:
            self._dialog_key_bindings = None

        self.current_dialog = float_dialog
        self.modal_open = True
        self.dialog_ok_handler = ok_handler

        try:
            self.app.invalidate()
        except Exception:
            pass

    def _close_dialog(self) -> None:
        """Close dialog"""
        if not self.modal_open or not self.current_dialog:
            return

        try:
            # Remove from root container if possible
            if hasattr(self.root, 'floats') and self.current_dialog in self.root.floats:
                self.root.floats.remove(self.current_dialog)
            else:
                # Fallback: remove from local floats list
                self.floats.remove(self.current_dialog)
        except ValueError:
            pass

        self.current_dialog = None
        self.modal_open = False
        self.dialog_ok_handler = None

        # Refocus appropriate element based on current phase
        try:
            if self.setup_visible:
                self.app.layout.focus(self.btn_peerA)
            else:
                self.app.layout.focus(self.input_field)
        except Exception:
            pass

        try:
            self.app.invalidate()
        except Exception:
            pass

    def _create_key_bindings(self) -> KeyBindings:
        """Key bindings"""
        kb = KeyBindings()

        # Note: TUI is part of tmux session, not standalone
        # Use Ctrl+b d to detach from tmux (which exits the whole cccc orchestrator)
        # Ctrl+C is reserved for interrupting CLI operations in peer panes

        @kb.add('c-q')
        def show_exit_help(event) -> None:
            """Show exit instructions"""
            self._write_timeline("To exit CCCC: Press Ctrl+b then d (detach from tmux session)", 'info')

        # Tab navigation (setup phase, non-modal) - unified with arrow keys
        @kb.add('tab', filter=Condition(lambda: self.setup_visible and not self.modal_open))
        def next_button(event) -> None:
            """Navigate to next item with Tab (unified with arrow keys)"""
            try:
                # Find current focused item using reliable has_focus check
                current_idx = -1
                for i, item in enumerate(self.navigation_items):
                    if item['type'] == 'button' and self.app.layout.has_focus(item['widget'].window):
                        current_idx = i
                        break
                    elif item['type'] == 'input' and self.app.layout.has_focus(item['widget']):
                        current_idx = i
                        break

                # Move to next item
                if current_idx >= 0:
                    next_idx = (current_idx + 1) % len(self.navigation_items)
                else:
                    next_idx = 0

                # Update focused index and move focus
                self.focused_option_index = next_idx
                next_item = self.navigation_items[next_idx]
                if next_item['type'] == 'button':
                    self.app.layout.focus(next_item['widget'])
                elif next_item['type'] == 'input':
                    self.app.layout.focus(next_item['widget'])
                self._update_focus_visual()
            except Exception:
                pass

        @kb.add('s-tab', filter=Condition(lambda: self.setup_visible and not self.modal_open))
        def prev_button(event) -> None:
            """Navigate to previous item with Shift+Tab (unified with arrow keys)"""
            try:
                # Find current focused item
                current_idx = -1
                for i, item in enumerate(self.navigation_items):
                    if item['type'] == 'button' and self.app.layout.has_focus(item['widget'].window):
                        current_idx = i
                        break
                    elif item['type'] == 'input' and self.app.layout.has_focus(item['widget']):
                        current_idx = i
                        break

                # Move to previous item
                if current_idx >= 0:
                    prev_idx = (current_idx - 1) % len(self.navigation_items)
                else:
                    prev_idx = len(self.navigation_items) - 1

                # Update focused index and move focus
                self.focused_option_index = prev_idx
                prev_item = self.navigation_items[prev_idx]
                if prev_item['type'] == 'button':
                    self.app.layout.focus(prev_item['widget'])
                elif prev_item['type'] == 'input':
                    self.app.layout.focus(prev_item['widget'])
                self._update_focus_visual()
            except Exception:
                pass

        # Arrow navigation in setup (unified with Tab behavior)
        @kb.add('down', filter=Condition(lambda: self.setup_visible and not self.modal_open))
        def next_option_arrow(event) -> None:
            """Navigate to next configuration option with Down arrow (moves cursor)"""
            try:
                # Update focused option index
                self.focused_option_index = (self.focused_option_index + 1) % len(self.navigation_items)

                # Move focus to the target item
                target_item = self.navigation_items[self.focused_option_index]
                if target_item['type'] == 'button':
                    self.app.layout.focus(target_item['widget'])
                elif target_item['type'] == 'input':
                    self.app.layout.focus(target_item['widget'])

                self._update_focus_visual()
            except Exception:
                pass

        @kb.add('up', filter=Condition(lambda: self.setup_visible and not self.modal_open))
        def prev_option_arrow(event) -> None:
            """Navigate to previous configuration option with Up arrow (moves cursor)"""
            try:
                # Update focused option index
                self.focused_option_index = (self.focused_option_index - 1) % len(self.navigation_items)

                # Move focus to the target item
                target_item = self.navigation_items[self.focused_option_index]
                if target_item['type'] == 'button':
                    self.app.layout.focus(target_item['widget'])
                elif target_item['type'] == 'input':
                    self.app.layout.focus(target_item['widget'])

                self._update_focus_visual()
            except Exception:
                pass

        # Value cycling with left/right arrows (dual interaction system) - only for buttons
        @kb.add('right', filter=Condition(lambda: self.setup_visible and not self.modal_open))
        def next_value_arrow(event) -> None:
            """Cycle to next value for focused option with Right arrow"""
            try:
                # Only cycle values if current item is a button, not an input field
                if self.focused_option_index < len(self.navigation_items):
                    current_item = self.navigation_items[self.focused_option_index]
                    if current_item['type'] == 'button':
                        current_config = current_item['name']
                        self.cycle_config_value(current_config, direction=1)
            except Exception:
                pass

        @kb.add('left', filter=Condition(lambda: self.setup_visible and not self.modal_open))
        def prev_value_arrow(event) -> None:
            """Cycle to previous value for focused option with Left arrow"""
            try:
                # Only cycle values if current item is a button, not an input field
                if self.focused_option_index < len(self.navigation_items):
                    current_item = self.navigation_items[self.focused_option_index]
                    if current_item['type'] == 'button':
                        current_config = current_item['name']
                        self.cycle_config_value(current_config, direction=-1)
            except Exception:
                pass

        # Enter to open detailed dialog for focused option
        @kb.add('enter', filter=Condition(lambda: self.setup_visible and not self.modal_open))
        def open_focused_option_dialog(event) -> None:
            """Open detailed selection dialog for focused option"""
            try:
                # Only open dialogs for buttons, not input fields
                if self.focused_option_index < len(self.navigation_items):
                    current_item = self.navigation_items[self.focused_option_index]
                    if current_item['type'] == 'button':
                        current_config = current_item['name']
                        if current_config in ['peerA', 'peerB']:
                            self._show_actor_dialog(current_config)
                        elif current_config == 'aux':
                            self._show_actor_dialog('aux')
                        elif current_config == 'foreman':
                            self._show_foreman_dialog()
                        elif current_config == 'self_opt':
                            # Toggle self-optimization on/off
                            self.config.self_opt_enabled = not self.config.self_opt_enabled
                            self.btn_self_opt.text = '[●] ON ' if self.config.self_opt_enabled else '[○] OFF'
                            self._refresh_ui()
                        elif current_config == 'mode':
                            self._show_mode_dialog()
            except Exception:
                pass

        # Command history navigation (runtime phase, not in search mode)
        @kb.add('up', filter=~Condition(lambda: self.setup_visible or self.modal_open or self.reverse_search_mode) & has_focus(self.input_field))
        def history_prev(event) -> None:
            if not self.command_history:
                return
            # Save current input when starting to navigate
            if self.history_index == -1:
                self.current_input = self.input_field.text
            # Navigate to previous command
            if self.history_index < len(self.command_history) - 1:
                self.history_index += 1
                self.input_field.text = self.command_history[-(self.history_index + 1)]
                # Move cursor to end
                self.input_field.buffer.cursor_position = len(self.input_field.text)

        @kb.add('down', filter=~Condition(lambda: self.setup_visible or self.modal_open or self.reverse_search_mode) & has_focus(self.input_field))
        def history_next(event) -> None:
            if self.history_index == -1:
                return
            # Navigate to next command
            self.history_index -= 1
            if self.history_index == -1:
                # Restore current input
                self.input_field.text = self.current_input
            else:
                self.input_field.text = self.command_history[-(self.history_index + 1)]
            # Move cursor to end
            self.input_field.buffer.cursor_position = len(self.input_field.text)

        # Clear screen (runtime phase)
        @kb.add('c-l', filter=~Condition(lambda: self.setup_visible or self.modal_open))
        def clear_screen(event) -> None:
            # Clear timeline with minimal message
            self.timeline.text = ''
            self.message_count = 0
            self._write_timeline("Screen cleared", 'system')
            self._write_timeline("Type /help for commands", 'info')

        # Timeline focus toggle (Ctrl+T to focus timeline for mouse scrolling)
        @kb.add('c-t', filter=~Condition(lambda: self.setup_visible or self.modal_open))
        def focus_timeline(event) -> None:
            """Focus timeline to enable mouse scrolling"""
            try:
                self.app.layout.focus(self.timeline)
            except Exception:
                pass

        # Close modal dialog with Esc (highest priority)
        # Use eager=True to ensure this runs before other escape handlers
        @kb.add('escape', filter=Condition(lambda: self.modal_open), eager=True)
        def close_modal(event) -> None:
            """Close any open modal dialog"""
            # If task detail dialog is open, use special close that preserves Level 1
            if self.task_detail_open:
                self._close_task_detail_dialog()
            else:
                self._close_dialog()
                try:
                    self.app.layout.focus(self.input_field)
                except Exception:
                    pass
        
        # Task detail navigation with arrow keys
        @kb.add('left', filter=Condition(lambda: self.task_detail_open and self.modal_open))
        def task_detail_prev(event) -> None:
            """Navigate to previous task in detail dialog"""
            try:
                if hasattr(self, '_current_task_ids') and hasattr(self, '_current_task_idx'):
                    if self._current_task_idx > 0:
                        prev_id = self._current_task_ids[self._current_task_idx - 1]
                        self._close_dialog()
                        self._show_task_detail_dialog(prev_id)
            except Exception:
                pass
        
        @kb.add('right', filter=Condition(lambda: self.task_detail_open and self.modal_open))
        def task_detail_next(event) -> None:
            """Navigate to next task in detail dialog"""
            try:
                if hasattr(self, '_current_task_ids') and hasattr(self, '_current_task_idx'):
                    if self._current_task_idx < len(self._current_task_ids) - 1:
                        next_id = self._current_task_ids[self._current_task_idx + 1]
                        self._close_dialog()
                        self._show_task_detail_dialog(next_id)
            except Exception:
                pass
        
        # Return to input from timeline (Esc)
        @kb.add('escape', filter=has_focus(self.timeline) & ~Condition(lambda: self.modal_open))
        def return_to_input(event) -> None:
            """Return focus to input field from timeline"""
            try:
                self.app.layout.focus(self.input_field)
            except Exception:
                pass

        # Timeline navigation
        @kb.add('pageup', filter=~Condition(lambda: self.setup_visible or self.modal_open))
        def page_up(event) -> None:
            """Scroll timeline up"""
            current_pos = self.timeline.buffer.cursor_position
            new_pos = max(0, current_pos - 500)
            self.timeline.buffer.cursor_position = new_pos

        @kb.add('pagedown', filter=~Condition(lambda: self.setup_visible or self.modal_open))
        def page_down(event) -> None:
            """Scroll timeline down"""
            current_pos = self.timeline.buffer.cursor_position
            max_pos = len(self.timeline.text)
            new_pos = min(max_pos, current_pos + 500)
            self.timeline.buffer.cursor_position = new_pos

        @kb.add('G', filter=~Condition(lambda: self.setup_visible or self.modal_open))
        def jump_to_bottom(event) -> None:
            """Jump to bottom of timeline (Shift+G, vim-style) - silent navigation"""
            self.timeline.buffer.cursor_position = len(self.timeline.text)

        @kb.add('g', 'g', filter=~Condition(lambda: self.setup_visible or self.modal_open))
        def jump_to_top(event) -> None:
            """Jump to top of timeline (gg, vim-style) - silent navigation"""
            self.timeline.buffer.cursor_position = 0

        # Task panel toggle (T key)
        @kb.add('T', filter=~Condition(lambda: self.setup_visible or self.modal_open) & ~has_focus(self.input_field))
        def toggle_task_panel(event) -> None:
            """Toggle task panel expanded/collapsed"""
            self._toggle_task_panel()

        # Task panel navigation (when expanded)
        # These bindings work even when input_field has focus - task panel is an overlay
        def is_task_panel_active():
            """Check if task panel is expanded and should receive navigation."""
            return (self.task_panel is not None 
                    and self.task_panel.expanded 
                    and not self.setup_visible 
                    and not self.modal_open)
        
        task_nav_filter = Condition(is_task_panel_active)

        @kb.add('down', filter=task_nav_filter)
        @kb.add('j', filter=task_nav_filter & ~has_focus(self.input_field))
        def task_select_next(event) -> None:
            """Move selection to next task"""
            if self.task_panel:
                self.task_panel.select_next()
                try:
                    self.app.invalidate()
                except Exception:
                    pass

        @kb.add('up', filter=task_nav_filter)
        @kb.add('k', filter=task_nav_filter & ~has_focus(self.input_field))
        def task_select_prev(event) -> None:
            """Move selection to previous task"""
            if self.task_panel:
                self.task_panel.select_prev()
                try:
                    self.app.invalidate()
                except Exception:
                    pass

        # Enter key: show task detail when panel is expanded
        # This works even when input is empty (panel takes priority)
        @kb.add('enter', filter=task_nav_filter & Condition(lambda: len(self.input_field.text.strip()) == 0))
        def task_show_detail(event) -> None:
            """Show detail of selected task as floating dialog"""
            if self.task_panel:
                task_id = self.task_panel.get_selected_task_id()
                if task_id:
                    # Show detail as floating dialog (Level 1 stays open)
                    self._show_task_detail_dialog(task_id)
                try:
                    self.app.invalidate()
                except Exception:
                    pass

        # Escape key: close task panel (highest priority when panel is open)
        @kb.add('escape', filter=task_nav_filter)
        def task_close_panel(event) -> None:
            """Close task panel with Esc"""
            if self.task_panel and self.task_panel.expanded:
                self.task_panel.toggle()
                try:
                    self.app.invalidate()
                except Exception:
                    pass

        # Standard editing shortcuts (runtime phase, input focused, not in search mode)
        @kb.add('c-a', filter=~Condition(lambda: self.setup_visible or self.modal_open or self.reverse_search_mode) & has_focus(self.input_field))
        def jump_to_start(event) -> None:
            self.input_field.buffer.cursor_position = 0

        @kb.add('c-e', filter=~Condition(lambda: self.setup_visible or self.modal_open or self.reverse_search_mode) & has_focus(self.input_field))
        def jump_to_end(event) -> None:
            self.input_field.buffer.cursor_position = len(self.input_field.text)

        @kb.add('c-w', filter=~Condition(lambda: self.setup_visible or self.modal_open or self.reverse_search_mode) & has_focus(self.input_field))
        def delete_word(event) -> None:
            buffer = self.input_field.buffer
            # Delete word before cursor
            pos = buffer.cursor_position
            text = buffer.text[:pos]
            # Find last space or start
            words = text.rstrip().rsplit(' ', 1)
            if len(words) == 1:
                # Delete everything before cursor
                buffer.text = buffer.text[pos:]
                buffer.cursor_position = 0
            else:
                # Delete last word
                new_text = words[0] + ' ' + buffer.text[pos:]
                buffer.text = new_text
                buffer.cursor_position = len(words[0]) + 1

        @kb.add('c-u', filter=~Condition(lambda: self.setup_visible or self.modal_open or self.reverse_search_mode) & has_focus(self.input_field))
        def delete_to_start(event) -> None:
            buffer = self.input_field.buffer
            pos = buffer.cursor_position
            buffer.text = buffer.text[pos:]
            buffer.cursor_position = 0

        @kb.add('c-k', filter=~Condition(lambda: self.setup_visible or self.modal_open or self.reverse_search_mode) & has_focus(self.input_field))
        def delete_to_end(event) -> None:
            buffer = self.input_field.buffer
            pos = buffer.cursor_position
            buffer.text = buffer.text[:pos]

        # Ctrl+V paste image from clipboard (or text if no image)
        # Note: In some terminals, Ctrl+V may be intercepted. Use eager=True to increase priority.
        @kb.add('c-v', filter=~Condition(lambda: self.setup_visible or self.modal_open or self.reverse_search_mode) & has_focus(self.input_field), eager=True)
        def paste_clipboard_image(event) -> None:
            """Check clipboard for image and insert path if found, otherwise paste text"""
            # Save images to unified .cccc/work/inbound-files/photos/ (same as IM bridges)
            import tempfile
            temp_dir = Path(tempfile.gettempdir()) / "cccc_clipboard_temp"
            
            try:
                image_path = get_clipboard_image(temp_dir)
            except Exception as e:
                self._write_timeline(f"Clipboard error: {e}", 'error')
                image_path = None
            
            if image_path:
                # Save to unified directory with metadata
                meta = save_image_for_cli(self.home, image_path)
                if meta:
                    # Insert path reference at cursor position
                    buffer = self.input_field.buffer
                    buffer.insert_text(f"@{meta['path']}")
                    self._write_timeline(f"Image ready: {meta['path']} ({meta['bytes']} bytes)", 'success')
                    # Clean up temp file
                    try:
                        Path(image_path).unlink()
                    except Exception:
                        pass
                else:
                    self._write_timeline("Failed to save image", 'error')
            else:
                # No image in clipboard, try to paste text (cross-platform)
                text = get_clipboard_text()
                if text:
                    buffer = self.input_field.buffer
                    buffer.insert_text(text)

        # Ctrl+R reverse search
        @kb.add('c-r', filter=~Condition(lambda: self.setup_visible or self.modal_open or self.reverse_search_mode) & has_focus(self.input_field))
        def start_reverse_search(event) -> None:
            """Start reverse history search"""
            self._start_reverse_search()
            self._write_timeline("Reverse search mode: type to search, Ctrl+R for next match, Enter to accept, Ctrl+G to cancel", 'info')

        # In reverse search mode, regular characters update the search
        @kb.add('<any>', filter=Condition(lambda: self.reverse_search_mode))
        def search_input(event) -> None:
            """Handle character input during reverse search"""
            char = event.data
            if char and char.isprintable():
                self.search_query += char
                self._perform_reverse_search(self.search_query)

        # Backspace in reverse search
        @kb.add('backspace', filter=Condition(lambda: self.reverse_search_mode))
        def search_backspace(event) -> None:
            """Handle backspace during reverse search"""
            if self.search_query:
                self.search_query = self.search_query[:-1]
                self._perform_reverse_search(self.search_query)

        # Ctrl+R again cycles through results
        @kb.add('c-r', filter=Condition(lambda: self.reverse_search_mode))
        def next_search_result(event) -> None:
            """Cycle to next search result"""
            if self.search_results and len(self.search_results) > 1:
                self.search_index = (self.search_index + 1) % len(self.search_results)
                self._update_search_prompt()

        # Enter accepts the search result
        @kb.add('enter', filter=Condition(lambda: self.reverse_search_mode))
        def accept_search(event) -> None:
            """Accept reverse search result"""
            self._exit_reverse_search(accept=True)
            # Don't submit immediately, let user edit if needed

        # Ctrl+G or Escape cancels search
        @kb.add('c-g', filter=Condition(lambda: self.reverse_search_mode))
        @kb.add('escape', filter=Condition(lambda: self.reverse_search_mode))
        def cancel_search(event) -> None:
            """Cancel reverse search"""
            self._exit_reverse_search(accept=False)

        # Enter to submit command (runtime phase, non-modal, not in search)
        # Tab key for completion in runtime mode
        @kb.add('tab', filter=~Condition(lambda: self.setup_visible or self.modal_open) & has_focus(self.input_field))
        def complete_command(event) -> None:
            """Trigger command completion with Tab"""
            buff = event.current_buffer
            if buff.complete_state:
                # Already showing completions, move to next
                buff.complete_next()
            else:
                # Start completion
                buff.start_completion(select_first=False)

        @kb.add('enter', filter=~Condition(lambda: self.setup_visible or self.modal_open or self.reverse_search_mode) & has_focus(self.input_field))
        def submit_command(event) -> None:
            text = self.input_field.text.strip()
            if text:
                # Add to history (avoid consecutive duplicates)
                if not self.command_history or self.command_history[-1] != text:
                    self.command_history.append(text)
                    # Limit history size
                    if len(self.command_history) > 1000:
                        self.command_history = self.command_history[-1000:]
            # Reset history navigation
            self.history_index = -1
            self.current_input = ''
            # Clear input and process
            self.input_field.text = ''
            if text:
                self._process_command(text)
            # Return focus to timeline for natural scrolling
            try:
                self.app.layout.focus(self.timeline)
            except Exception:
                pass

        # Smart focus: any printable key when timeline focused -> auto-switch to input
        from prompt_toolkit.keys import Keys
        @kb.add(Keys.Any, filter=~Condition(lambda: self.setup_visible or self.modal_open) & has_focus(self.timeline))
        def auto_focus_input(event) -> None:
            """Auto-focus input when user starts typing (2025 TUI UX)"""
            if event.data and len(event.data) == 1 and event.data.isprintable():
                try:
                    self.app.layout.focus(self.input_field)
                    self.input_field.buffer.insert_text(event.data)
                except Exception:
                    pass

        # Help toggle (F1)
        @kb.add('f1')
        def toggle_help(event) -> None:
            """Toggle help hints"""
            self.help_hint_visible = not self.help_hint_visible
            if self.help_hint_visible:
                self._write_timeline("Help hints: ON • Use ↑↓ to navigate, ←→ to change values, Enter for details", 'info')
            else:
                self._write_timeline("Help hints: OFF", 'info')

        return kb

    async def refresh_loop(self) -> None:
        """Background refresh with connection monitoring"""
        seen_messages = set()
        orchestrator_dead_count = 0  # Track consecutive failed liveness checks

        while True:
            try:
                if not self.setup_visible:
                    # Check orchestrator liveness via PID file (Unix standard approach)
                    # This is far superior to file-based heartbeats:
                    # - Zero disk I/O during runtime (os.kill is a syscall only)
                    # - Instant detection when process dies
                    # - No disk wear from frequent writes
                    try:
                        pid_file = self.home / "state" / "orchestrator.pid"
                        if pid_file.exists():
                            try:
                                pid_text = pid_file.read_text(encoding='utf-8', errors='replace').strip()
                                pid = int(pid_text)
                                # Use os.kill(pid, 0) to check if process exists
                                # Signal 0 doesn't send a signal, just checks process existence
                                os.kill(pid, 0)
                                # Process is alive, reset counter
                                orchestrator_dead_count = 0
                            except (ValueError, ProcessLookupError, PermissionError):
                                # Invalid PID or process doesn't exist
                                orchestrator_dead_count += 1
                                if orchestrator_dead_count >= 3:  # 3 checks = 6 seconds
                                    self._write_timeline("Orchestrator process exited. Shutting down TUI...", 'error')
                                    await asyncio.sleep(1.0)
                                    self.app.exit()
                                    return
                        else:
                            # PID file doesn't exist yet (startup) or orchestrator exited
                            orchestrator_dead_count += 1
                            if orchestrator_dead_count >= 5:  # More lenient during startup (10 seconds)
                                self._write_timeline("Orchestrator PID file not found. Exiting TUI...", 'error')
                                await asyncio.sleep(1.0)
                                self.app.exit()
                                return
                    except Exception as e:
                        # Unexpected error during liveness check
                        orchestrator_dead_count += 1
                        if orchestrator_dead_count >= 5:
                            self._write_timeline(f"Lost connection to orchestrator: {e}. Exiting TUI...", 'error')
                            await asyncio.sleep(1.0)
                            self.app.exit()
                            return

                    # Check for command replies
                    self._check_tui_replies()

                    # Refresh timeline from outbox.jsonl
                    try:
                        outbox = self.home / "state" / "outbox.jsonl"
                        if outbox.exists():
                            self.orchestrator_connected = True
                            lines = outbox.read_text(encoding='utf-8', errors='replace').splitlines()[-100:]

                            # Process new messages
                            for ln in lines:
                                if not ln.strip():
                                    continue
                                try:
                                    ev = json.loads(ln)
                                    # Create unique message ID
                                    msg_id = f"{ev.get('from')}:{ev.get('text', '')[:50]}"

                                    if msg_id in seen_messages:
                                        continue

                                    if ev.get('type') in ('to_user', 'to_peer_summary'):
                                        frm = ev.get('from', ev.get('peer', '?')).lower()
                                        text = ev.get('text', '')

                                        # Determine message type based on source
                                        if frm == 'peera' or frm == 'a':
                                            msg_type = 'peerA'
                                            display_name = 'PeerA'
                                        elif frm == 'peerb' or frm == 'b':
                                            msg_type = 'peerB'
                                            display_name = 'PeerB'
                                        elif frm == 'system':
                                            msg_type = 'system'
                                            display_name = 'System'
                                        else:
                                            msg_type = 'info'
                                            display_name = frm.upper()

                                        # Add message
                                        self._write_timeline(f"{display_name}: {text}", msg_type)
                                        seen_messages.add(msg_id)

                                        # Keep set size manageable
                                        if len(seen_messages) > 200:
                                            seen_messages = set(list(seen_messages)[-100:])
                                except Exception:
                                    pass
                        else:
                            self.orchestrator_connected = False

                    except Exception:
                        self.orchestrator_connected = False

                        # Update status panel
                        self._update_status()

                # Request a redraw so footer warnings/status reflect latest files
                try:
                    self.app.invalidate()
                except Exception:
                    pass

                await asyncio.sleep(2.0)

            except asyncio.CancelledError:
                # Properly handle cancellation
                break
            except Exception:
                # Log error but continue loop
                await asyncio.sleep(2.0)

    def _check_residual_inbox(self) -> None:
        """Check for residual inbox messages before orchestrator launch.

        This check runs on EVERY launch to detect messages left unprocessed
        from previous session (e.g., after crashes or abnormal shutdown).

        If inbox is empty, check completes instantly (<10ms).
        If messages found, shows dialog for user to Process or Discard.
        """
        try:
            # Import mailbox functions
            import sys
            sys.path.insert(0, str(self.home))
            from mailbox import ensure_mailbox

            ensure_mailbox(self.home)

            # Count inbox files
            cntA = 0
            cntB = 0

            # Check PeerA inbox
            ibA = self.home / "mailbox" / "peerA" / "inbox"
            if ibA.exists():
                cntA = len([f for f in ibA.iterdir() if f.is_file()])

            # Check PeerB inbox
            ibB = self.home / "mailbox" / "peerB" / "inbox"
            if ibB.exists():
                cntB = len([f for f in ibB.iterdir() if f.is_file()])

            # If no residual messages, continue launch directly
            if cntA == 0 and cntB == 0:
                self._continue_launch()
                return

            # Show dialog for user to decide how to handle residual messages
            self._show_inbox_cleanup_dialog(cntA, cntB)

        except Exception as e:
            # Log error but don't block launch (use module-level imports to avoid UnboundLocalError)
            try:
                ledger_path = self.home / "state" / "ledger.jsonl"
                ledger_path.parent.mkdir(parents=True, exist_ok=True)
                with ledger_path.open('a', encoding='utf-8') as f:
                    error_entry = {
                        "from": "system",
                        "kind": "startup-inbox-check-error",
                        "error": str(e)[:200],
                        "ts": time.time()
                    }
                    f.write(json.dumps(error_entry, ensure_ascii=False) + '\n')
            except Exception:
                pass
            # Proceed despite error
            self._continue_launch()

    def _show_inbox_cleanup_dialog(self, cntA: int, cntB: int) -> None:
        """Show inbox cleanup dialog with user-friendly messaging

        Args:
            cntA: Count of messages in PeerA inbox
            cntB: Count of messages in PeerB inbox
        """
        if self.modal_open:
            return

        total = cntA + cntB
        msg_lines = [
            f"📬 Found {total} unprocessed message(s) from previous session",
            ""
        ]
        if cntA > 0:
            msg_lines.append(f"  • PeerA inbox: {cntA} message(s)")
        if cntB > 0:
            msg_lines.append(f"  • PeerB inbox: {cntB} message(s)")
        msg_lines.extend([
            "",
            "These messages were not delivered to agents before CCCC stopped.",
            "",
            "📌 What should we do with them?",
            "",
            "  ✓ Process Now:",
            "    Agents will receive and respond to these messages",
            "    (Messages stay in inbox and will be delivered)",
            "",
            "  ✗ Discard:",
            "    Archive these messages to processed/ folder",
            "    (Agents won't see them, workflow starts fresh)",
        ])

        alert_text = "\n".join(msg_lines)

        def on_process() -> None:
            """Process - keep messages in inbox for delivery"""
            self._cleanup_inbox_messages(discard=False)
            self._close_dialog()
            self._write_timeline(f"Processing {total} pending message(s)", 'info')
            self._continue_launch()

        def on_discard() -> None:
            """Discard - move messages to processed/ directory"""
            self._cleanup_inbox_messages(discard=True)
            self._close_dialog()
            self._write_timeline(f"Discarded {total} message(s) - starting fresh", 'info')
            self._continue_launch()

        # Create clear 2-option dialog
        dialog = Dialog(
            title="📬 Unprocessed Messages Found",
            body=HSplit([
                Label(text=alert_text, style='class:info'),
            ]),
            buttons=[
                Button('✓ Process Now', handler=on_process, width=18),
                Button('✗ Discard', handler=on_discard, width=18),
            ],
            width=Dimension(min=70, max=90, preferred=80),
            modal=True
        )

        self._open_dialog(dialog, on_process)  # Default is Process

    def _check_tui_replies(self) -> None:
        """Monitor tui-replies.jsonl for command responses"""
        reply_file = self.home / "state" / "tui-replies.jsonl"
        if not reply_file.exists():
            return

        # Track last read position to avoid re-reading
        if not hasattr(self, '_reply_file_pos'):
            # Start from END of file to skip old replies from previous sessions
            try:
                with reply_file.open('r', encoding='utf-8') as f:
                    f.seek(0, 2)  # Seek to end (2 = SEEK_END)
                    self._reply_file_pos = f.tell()
            except Exception:
                self._reply_file_pos = 0
            return  # Skip reading on first call (just set position)

        try:
            with reply_file.open('r', encoding='utf-8') as f:
                # Seek to last position
                f.seek(self._reply_file_pos)
                
                # Read new lines
                for line in f:
                    line = line.strip()
                    if not line:
                        continue
                    
                    try:
                        reply = json.loads(line)
                        ok = reply.get('ok', True)
                        message = reply.get('message', 'Done')
                        
                        # Display reply in timeline
                        style = 'success' if ok else 'error'
                        self._write_timeline(f"← {message}", style)
                        
                    except json.JSONDecodeError:
                        pass  # Skip malformed lines
                
                # Update position
                self._reply_file_pos = f.tell()
        except Exception:
            pass  # Silent fail - don't break TUI

    def _cleanup_inbox_messages(self, discard: bool) -> None:
        """Clean up inbox messages based on user choice.

        Args:
            discard: If True, move to processed/; if False, keep in inbox/
        """
        if not discard:
            # User chose to keep messages - do nothing
            return

        try:
            # Move messages to processed directory
            moved_count = 0

            for peer in ["peerA", "peerB"]:
                inbox_dir = self.home / "mailbox" / peer / "inbox"
                processed_dir = self.home / "mailbox" / peer / "processed"

                if not inbox_dir.exists():
                    continue

                processed_dir.mkdir(parents=True, exist_ok=True)

                for msg_file in inbox_dir.iterdir():
                    if msg_file.is_file():
                        try:
                            msg_file.rename(processed_dir / msg_file.name)
                            moved_count += 1
                        except Exception:
                            pass

            # Log the cleanup action
            self._log_inbox_cleanup(moved_count, "discarded")

        except Exception as e:
            # Log error but don't break
            try:
                import json, time
                ledger_path = self.home / "state" / "ledger.jsonl"
                ledger_path.parent.mkdir(parents=True, exist_ok=True)
                with ledger_path.open('a', encoding='utf-8') as f:
                    f.write(json.dumps({
                        "from": "system",
                        "kind": "inbox-cleanup-error",
                        "error": str(e)[:200],
                        "ts": time.time()
                    }, ensure_ascii=False) + '\n')
            except Exception:
                pass

    def _log_inbox_cleanup(self, count: int, action: str) -> None:
        """Log inbox cleanup action"""
        try:
            import json, time
            ledger_path = self.home / "state" / "ledger.jsonl"
            ledger_path.parent.mkdir(parents=True, exist_ok=True)
            with ledger_path.open('a', encoding='utf-8') as f:
                f.write(json.dumps({
                    "from": "system",
                    "kind": "inbox-cleanup",
                    "action": action,
                    "count": count,
                    "ts": time.time()
                }, ensure_ascii=False) + '\n')
        except Exception:
            pass


def run(home: Path) -> None:
    """Entry point - simplified for stability"""
    try:
        print("Starting CCCC TUI...")
        app = CCCCSetupApp(home)

        # Write ready flag
        try:
            p = home / "state" / "tui.ready"
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(str(int(time.time())))
        except Exception:
            pass

        # Launch with refresh loop for live updates - simplified approach
        print("Launching application with refresh loop...")

        # Use prompt_toolkit's built-in async support
        # Start the refresh loop as a background task and run the app
        async def main():
            # Get event loop for signal handling
            loop = asyncio.get_running_loop()

            # Setup signal handlers for graceful shutdown
            # This ensures prompt_toolkit can restore terminal state when receiving signals
            def handle_signal(signame):
                print(f"\n[TUI] Received {signame}, shutting down gracefully...")
                app.app.exit()

            # Register handlers for common termination signals
            import signal
            for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
                loop.add_signal_handler(sig, lambda s=sig: handle_signal(signal.Signals(s).name))

            # Start refresh loop in background
            refresh_task = asyncio.create_task(app.refresh_loop())

            try:
                # Run the main application
                await app.app.run_async()
            finally:
                # Clean up when app exits
                refresh_task.cancel()
                try:
                    await refresh_task
                except asyncio.CancelledError:
                    pass

        # Run the main async function
        asyncio.run(main())

    except Exception as e:
        print(f"Error starting TUI: {e}")
        import traceback
        traceback.print_exc()
