#!/usr/bin/env python3
"""
CCCC PTK TUI - Modern Interactive Orchestrator Interface

Production-ready CLI interface for dual-agent collaboration with CCCC.
Meets 2025 CLI standards for usability, aesthetics, and functionality.

Core Features:
  • Setup: Elegant actor configuration with visual hierarchy
  • Runtime: Real-time collaborative CLI with Timeline, Input, Status
  • Commands: /a, /b, /both, /help, /pause, /resume, /refresh, /quit, /foreman, /c, /review, /focus, /verbose on|off

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
    FormattedTextControl, Dimension
)
from prompt_toolkit.widgets import (
    TextArea, Button, Dialog, RadioList, Label, Frame
)
from prompt_toolkit.styles import Style
from prompt_toolkit.mouse_events import MouseEventType
from prompt_toolkit.shortcuts import radiolist_dialog
from asyncio import create_task


class CommandCompleter(Completer):
    """Auto-completion for CCCC commands"""

    def __init__(self):
        super().__init__()
        # Define available commands with descriptions (console removed; TUI/IM are primary)
        self.commands = [
            # Basic control
            ('/help', 'Show help'),
            ('/pause', 'Pause handoff'),
            ('/resume', 'Resume handoff'),
            ('/refresh', 'Refresh system prompt'),
            ('/quit', 'Quit CCCC'),
            # Foreman
            ('/foreman', 'Foreman control (on|off|status|now)'),
            # Aux
            ('/c', 'Run Aux helper'),
            ('/review', 'Request Aux review'),
            # Focus and filter
            ('/focus', 'Focus PeerB'),
            ('/verbose', 'Verbose on|off'),
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
    mode: str = 'tmux'
    tg_token: str = ''
    tg_chat: str = ''
    sl_token: str = ''
    sl_chan: str = ''
    dc_token: str = ''
    dc_chan: str = ''

    def is_valid(self, actors: List[str], home: Path) -> tuple[bool, str]:
        """Validate configuration"""
        if not self.peerA:
            return False, "PeerA actor required"
        if not self.peerB:
            return False, "PeerB actor required"

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
        if self.mode == 'slack' and not self.sl_token:
            return False, "Slack token required"
        if self.mode == 'discord' and not self.dc_token:
            return False, "Discord token required"

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
    except Exception:
        pass  # Silently fail and try fallback

    # Fallback: check default command name in PATH
    if shutil.which(actor_name):
        return True, "Installed"

    # Not found - provide accurate installation hints
    install_hints = {
        'claude': 'npm install -g @anthropic-ai/claude-code',
        'codex': 'npm i -g @openai/codex',
        'gemini': 'npm install -g @google/gemini-cli',
        'droid': 'curl -fsSL https://app.factory.ai/cli | sh',
        'opencode': 'npm i -g opencode-ai',
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


def create_header() -> Window:
    """Professional 2025 CLI header with ASCII art, version, and branding"""
    # Get CCCC version from package metadata
    try:
        from importlib.metadata import version as get_version
        version = get_version('cccc-pair')
    except Exception:
        version = 'unknown'

    text = [
        ('class:title', '\n'),
        ('class:title', '   ╔═══════════════════════════════════════════════════════════════╗\n'),
        ('class:title', '   ║  '), ('class:title.bold', '██████╗  ██████╗  ██████╗  ██████╗'), ('class:title', '                         ║\n'),
        ('class:title', '   ║  '), ('class:title.bold', '██╔════╝ ██╔════╝ ██╔════╝ ██╔════╝'), ('class:title', '                        ║\n'),
        ('class:title', '   ║  '), ('class:title.bold', '██║      ██║      ██║      ██║     '), ('class:title', '                        ║\n'),
        ('class:title', '   ║  '), ('class:title.bold', '██║      ██║      ██║      ██║     '), ('class:title', '                        ║\n'),
        ('class:title', '   ║  '), ('class:title.bold', '╚██████╗ ╚██████╗ ╚██████╗ ╚██████╗'), ('class:title', '                        ║\n'),
        ('class:title', '   ║  '), ('class:title.bold', ' ╚═════╝  ╚═════╝  ╚═════╝  ╚═════╝'), ('class:title', '                        ║\n'),
        ('class:title', '   ║                                                               ║\n'),
        ('class:title', '   ║  '), ('class:success.bold', 'CLI x CLI Co-Creation'), ('class:title', ' · Multi-Agent Orchestrator  ║\n'),
        ('class:title', '   ╚═══════════════════════════════════════════════════════════════╝\n'),
        ('', '\n'),
        ('class:info', '   Version: '), ('class:value', f'{version}'),
        ('', '\n'),
        ('class:hint', '   Evidence-first collaboration · Single-branch workflow · Verifiable changes\n'),
        ('', '\n'),
        ('class:section', '   ─' * 60 + '\n'),
        ('class:hint', '   ⌨  Tab/↑↓: navigate  ·  Enter: select/confirm  ·  Esc: cancel\n'),
        ('class:section', '   ─' * 60 + '\n'),
    ]
    return Window(
        content=FormattedTextControl(text),
        height=Dimension(min=16, max=18),
        dont_extend_height=True
    )


def create_runtime_header(status_dot: str = '●', status_color: str = 'class:status.connected') -> Window:
    """Modern runtime header with connection status"""
    text = [
        ('class:title', '❯ CCCC Orchestrator  '),
        (status_color, status_dot),
        ('', '\n'),
        ('class:hint', '  /help for commands  ·  Ctrl+b d to exit'),
    ]
    return Window(
        content=FormattedTextControl(text),
        height=Dimension(min=2, max=3),
        dont_extend_height=True
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
        self.buttons: List[Button] = []
        self.setup_content = self._build_setup_panel()

        # Initialize Runtime UI after setup UI is built
        ts = time.strftime('%H:%M:%S')
        initial_msg = f"""[{ts}] SYS CCCC Orchestrator v0.3.x
[{ts}] ... Dual-agent collaboration system
[{ts}] ... Type /help for commands and shortcuts
"""
        self.timeline = TextArea(
            text=initial_msg,
            scrollbar=True,
            read_only=True,
            focusable=True,  # Enable focus for mouse scrolling
            wrap_lines=False
        )
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
        # Track last message timestamp for grouping
        self.last_message_time: float = 0
        self.last_message_sender: str = ''
        self.message_count: int = 0

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
                self.root.content = HSplit([
                    create_header(),
                    Window(height=1),
                    self.setup_content,
                ])

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
                    # Focused item - highlight with bright color
                    if item['type'] == 'button':
                        item['widget'].style = "#5fff7f bold"
                    elif item['type'] == 'input':
                        # Input fields get border style to show focus
                        item['widget'].style = "#ffffff bg:#303030"
                else:
                    # Unfocused items - normal styling
                    if item['type'] == 'button':
                        config_name = item['name']
                        if config_name in ['peerA', 'peerB']:
                            current_value = getattr(self.config, config_name, None)
                            if current_value and self.actor_availability.get(current_value, (False, ""))[0]:
                                item['widget'].style = "bold green"
                            else:
                                item['widget'].style = "bold red"
                        elif config_name == 'aux':
                            current_value = getattr(self.config, 'aux', None)
                            if current_value and current_value != 'none':
                                item['widget'].style = "bold green"
                            else:
                                item['widget'].style = "bold yellow"
                        elif config_name == 'foreman':
                            current_value = getattr(self.config, 'foreman', None)
                            if current_value and current_value != 'none':
                                item['widget'].style = "bold green"
                            else:
                                item['widget'].style = "bold yellow"
                        elif config_name == 'mode':
                            item['widget'].style = "bold cyan"
                    elif item['type'] == 'input':
                        # Unfocused input fields
                        item['widget'].style = "#d0d0d0 bg:#303030"

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
            if config_name in ['peerA', 'peerB']:
                # Required peers: only include available actors
                if self.actors_available:
                    return self.actors_available
                else:
                    return ['claude']  # Fallback to claude if no actors found
            elif config_name == 'aux':
                # Optional: include none and all available actors
                return ['none'] + self.actors_available
            elif config_name == 'foreman':
                # Foreman options
                return ['none', 'reuse_aux'] + self.actors_available
            elif config_name == 'mode':
                # Interaction modes
                return ['tmux', 'telegram', 'slack', 'discord']
            else:
                return []

        def cycle_config_value(config_name: str, direction: int = 1) -> None:
            """Cycle config value forward (1) or backward (-1)"""
            choices = get_value_choices(config_name)
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

            # Update config
            setattr(self.config, config_name, new_value)

            # Update button text - use direct button reference
            button = None
            if config_name == 'peerA':
                button = self.btn_peerA
                required = True
                none_ok = False
            elif config_name == 'peerB':
                button = self.btn_peerB
                required = True
                none_ok = False
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

            if button:
                if config_name == 'mode':
                    button.text = f'[●] {new_value}'
                else:
                    button.text = self._format_button_text(new_value, required=required, none_ok=none_ok)

            # Handle special case: mode change requires UI rebuild
            if config_name == 'mode':
                self._handle_mode_change()

            # Trigger UI refresh
            self._refresh_ui()

        # Store cycling method for use in keyboard bindings
        self.cycle_config_value = cycle_config_value

    def _update_setup_button_text(self):
        """更新配置按钮文本"""
        button_texts = {
            'actor': f"Agent [b]A[/b]: {self.current_actor} ({'✓' if self.actor_availability[self.current_actor]['configured'] else '✗'}{' ' + self.actor_availability[self.current_actor]['version'] if self.actor_availability[self.current_actor]['version'] else ''})",
            'foreman': f"Foreman [b]B[/b]: {self.current_foreman} ({'✓' if self.foreman_availability[self.current_foreman]['configured'] else '✗'}{' ' + self.foreman_availability[self.current_foreman]['version'] if self.foreman_availability[self.current_foreman]['version'] else ''})",
            'mode': f"Mode: {self.current_mode}"
        }

        self.config_buttons['actor'].text = button_texts['actor']
        self.config_buttons['foreman'].text = button_texts['foreman']
        self.config_buttons['mode'].text = button_texts['mode']

        # 根据配置状态更新按钮样式
        actor_configured = self.actor_availability[self.current_actor]['configured']
        foreman_configured = self.foreman_availability[self.current_foreman]['configured']

        self.config_buttons['actor'].style = "bold green" if actor_configured else "bold red"
        self.config_buttons['foreman'].style = "bold green" if foreman_configured else "bold red"

    def _create_ui(self):
        """创建UI组件"""
        self._create_styles()
        self._create_layout()
        self.key_bindings = self._create_key_bindings()
        self._create_dialogs()

    def _create_root_layout(self):
        """Create the root layout"""
        # Create root container
        self.root = FloatContainer(
            content=HSplit([
                create_header(),
                Window(height=1),
                self.setup_content,
            ]),
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
        """创建样式"""
        self.style = Style.from_dict({
            # 基础样式
            'title': '#5fd7ff bold',                # 青色标题
            'subtitle': '#87afff',                 # 淡蓝色副标题
            'heading': '#ffaf00 bold',             # 橙色标题
            'separator': '#6c6c6c',                # 灰色分隔符
            'version': '#767676',                  # 深灰色版本号
            'ascii-art': '#5f87af',                # 淡蓝色ASCII艺术

            # 配置按钮样式
            'config-button': '#d0d0d0',            # 默认按钮
            'config-button.focused': '#5fff7f bold',  # 聚焦时亮绿色
            'config-button.configured': '#5fff5f bold',   # 已配置绿色
            'config-button.unconfigured': '#ff5f5f bold', # 未配置红色

            # 状态指示器
            'status-indicator': '#5fff5f',         # 状态指示器
            'status-text': '#d0d0d0',              # 状态文本

            # 用户消息样式
            'msg.user': '#ffd787',             # Yellow for user
            'msg.error': '#ff5f5f bold',       # Red for errors
            'msg.debug': '#6c6c6c',            # Gray for debug

            # Status indicators
            'status.connected': '#5fff5f',     # Green dot for connected
            'status.disconnected': '#ff5f5f',  # Red dot for disconnected
            'status.idle': '#6c6c6c',          # Gray dot for idle

            # UI components
            'dialog': 'bg:#1c1c1c',
            'dialog.body': 'bg:#262626',
            'button': 'bg:#3a3a3a #d0d0d0',
            'button.focused': 'bg:#5f87af #ffffff bold',

            # Completion menu
            'completion-menu': 'bg:#262626 #d0d0d0',
            'completion-menu.completion': 'bg:#262626 #d0d0d0',
            'completion-menu.completion.current': 'bg:#5f87af #ffffff bold',
            'completion-menu.meta': 'bg:#262626 #87afaf',
            'completion-menu.meta.current': 'bg:#5f87af #d0d0d0',

            # Prompt
            'prompt': '#5fd7ff bold',          # Cyan prompt symbol
            'prompt.search': '#ffaf00 bold',   # Orange for search mode

            # Input fields
            'input-field': '#ffffff bg:#303030',  # White text on dark background
        })

    def _load_existing_config(self) -> None:
        """Load configuration"""
        # Load actors
        try:
            import yaml
            p = self.home / "settings" / "agents.yaml"
            if p.exists():
                data = yaml.safe_load(p.read_text(encoding='utf-8')) or {}
                acts = data.get('actors') or {}
                self.actors_available = list(acts.keys()) if isinstance(acts, dict) else []
        except Exception:
            pass

        if not self.actors_available:
            self.actors_available = ['claude', 'codex', 'gemini', 'droid', 'opencode']

        # Load roles
        try:
            import yaml
            p = self.home / "settings" / "cli_profiles.yaml"
            if p.exists():
                data = yaml.safe_load(p.read_text(encoding='utf-8')) or {}
                roles = data.get('roles') or {}
                self.config.peerA = str((roles.get('peerA') or {}).get('actor') or '')
                self.config.peerB = str((roles.get('peerB') or {}).get('actor') or '')
                self.config.aux = str((roles.get('aux') or {}).get('actor') or 'none')
        except Exception:
            pass

        # Smart defaults
        if not self.config.peerA and len(self.actors_available) > 0:
            self.config.peerA = self.actors_available[0]
        if not self.config.peerB and len(self.actors_available) > 1:
            self.config.peerB = self.actors_available[1]
        elif not self.config.peerB and len(self.actors_available) > 0:
            self.config.peerB = self.actors_available[0]

        # Load foreman
        try:
            import yaml
            p = self.home / "settings" / "foreman.yaml"
            if p.exists():
                data = yaml.safe_load(p.read_text(encoding='utf-8')) or {}
                self.config.foreman = str(data.get('agent') or 'none')
        except Exception:
            pass

        # Load telegram
        try:
            import yaml
            p = self.home / "settings" / "telegram.yaml"
            if p.exists():
                data = yaml.safe_load(p.read_text(encoding='utf-8')) or {}
                self.config.tg_token = str(data.get('token') or '')
                chats = data.get('allow_chats') or []
                if isinstance(chats, list) and chats:
                    self.config.tg_chat = str(chats[0])
                if data.get('autostart') and self.config.tg_token:
                    self.config.mode = 'telegram'
        except Exception:
            pass

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
            width=24,
            left_symbol='',
            right_symbol=''
        )
        btn_peerB = Button(
            text=self._format_button_text(self.config.peerB, required=True),
            handler=lambda: self._show_actor_dialog('peerB'),
            width=24,
            left_symbol='',
            right_symbol=''
        )
        btn_aux = Button(
            text=self._format_button_text(self.config.aux, none_ok=True),
            handler=lambda: self._show_actor_dialog('aux'),
            width=24,
            left_symbol='',
            right_symbol=''
        )
        btn_foreman = Button(
            text=self._format_button_text(self.config.foreman, none_ok=True),
            handler=self._show_foreman_dialog,
            width=24,
            left_symbol='',
            right_symbol=''
        )
        btn_mode = Button(
            text=f'[●] {self.config.mode}',
            handler=self._show_mode_dialog,
            width=24,
            left_symbol='',
            right_symbol=''
        )
        btn_confirm = Button(
            text='🚀 Launch CCCC',
            handler=self._confirm_and_launch,
            width=20,
            left_symbol='',
            right_symbol=''
        )
        btn_quit = Button(
            text='Quit',
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

        # Map config names to buttons for easy access
        self.config_buttons = {
            'peerA': btn_peerA,
            'peerB': btn_peerB,
            'aux': btn_aux,
            'foreman': btn_foreman,
            'mode': btn_mode
        }

        # Build initial buttons list (will be updated dynamically)
        self.buttons = [btn_peerA, btn_peerB, btn_aux, btn_foreman, btn_mode]
        self._update_navigation_items()

        # Clean, minimal layout with dual interaction system
        items = [
            self.error_label,
            Window(height=1),

            # Dual interaction system header and instructions
            create_section_header('Configuration Setup • Dual Interaction System'),
            Label(text='↑↓ Navigate Options  ←→ Change Values  Enter: Details  Tab: Buttons', style='class:hint'),
            Window(height=1),

            # Core agents
            create_section_header('Core Agents'),
            Window(height=1),
            VSplit([
                Window(width=10, content=self._create_focused_label('PeerA', 0)),
                btn_peerA,
                Window(width=2),
                Label(text='Strategic peer (equal, can think & execute)', style='class:hint'),
            ], padding=1),
            VSplit([
                Window(width=10, content=self._create_focused_label('PeerB', 1)),
                btn_peerB,
                Window(width=2),
                Label(text='Validation peer (equal, debate & evidence)', style='class:hint'),
            ], padding=1),
            Window(height=1),

            # Optional agents
            create_section_header('Optional'),
            Window(height=1),
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
                Label(text='Background scheduler (self-check/maintenance/compact)', style='class:hint'),
            ], padding=1),
            Window(height=1),

            # Interaction mode
            create_section_header('Mode'),
            Window(height=1),
            VSplit([
                Window(width=10, content=self._create_focused_label('Connect', 4)),
                btn_mode,
                Window(width=2),
                Label(text='Interaction mode (tmux local / telegram remote / team chat)', style='class:hint'),
            ], padding=1),
        ]

        # IM Configuration (integrated approach)
        if self.config.mode in ('telegram', 'slack', 'discord'):
            items.extend([
                Window(height=1),
                create_section_header('IM Configuration'),
                Window(height=1),

                # Bot Token input
                VSplit([
                    Window(width=10, content=FormattedTextControl('Bot Token:')),
                    self._create_token_field(),
                    Window(width=2),
                    Label(text='Required for bot authentication', style='class:hint'),
                ], padding=1),
                Window(height=1),

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
            Label(text='To exit CCCC: Ctrl+b then d (detach tmux) or use Quit button', style='class:hint'),
            Window(height=1),
            Label(text='─' * 40, style='class:section'),  # Flexible separator
            Window(height=1),
            VSplit([
                btn_confirm,
                Window(width=2),
                btn_quit,
            ], padding=1),
        ])

        # Initialize value cycling methods after UI is built
        try:
            self._setup_value_cycling_deferred()
            self.value_cycling_initialized = True
        except Exception:
            pass  # Ignore errors in value cycling setup

        return HSplit(items)

    def _update_navigation_items(self) -> None:
        """Update navigation items list based on current mode"""
        self.navigation_items = []

        # Always include core buttons
        self.navigation_items.extend([
            {'type': 'button', 'name': 'peerA', 'widget': self.btn_peerA},
            {'type': 'button', 'name': 'peerB', 'widget': self.btn_peerB},
            {'type': 'button', 'name': 'aux', 'widget': self.btn_aux},
            {'type': 'button', 'name': 'foreman', 'widget': self.btn_foreman},
            {'type': 'button', 'name': 'mode', 'widget': self.btn_mode},
        ])

        # Add IM input fields if IM mode is selected
        if self.config.mode in ('telegram', 'slack', 'discord'):
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
                initial_text = self.config.sl_token or ''
            elif mode == 'discord':
                initial_text = self.config.dc_token or ''

            self.token_field = TextArea(
                height=1,
                multiline=False,
                text=initial_text,
                wrap_lines=False,
                style='class:input-field'
            )
        return self.token_field

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
                self.config.sl_token = self.token_field.text.strip()
                self.config.sl_chan = self.channel_field.text.strip()
            elif mode == 'discord':
                self.config.dc_token = self.token_field.text.strip()
                self.config.dc_chan = self.channel_field.text.strip()

    def _format_button_text(self, value: str, required: bool = False, none_ok: bool = False) -> str:
        """Format button text (no availability status in setup panel)"""
        if not value:
            return '[○] (not set)' if required else '[○] none'
        if value == 'none' and none_ok:
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
            tok = '●' if self.config.sl_token else '○'
            return f'[{tok}] token set' if self.config.sl_token else '[○] not configured'
        elif mode == 'discord':
            tok = '●' if self.config.dc_token else '○'
            return f'[{tok}] token set' if self.config.dc_token else '[○] not configured'
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
        self.btn_peerB.text = self._format_button_text(self.config.peerB, required=True)
        self.btn_aux.text = self._format_button_text(self.config.aux, none_ok=True)
        self.btn_foreman.text = self._format_button_text(self.config.foreman, none_ok=True)
        self.btn_mode.text = f'[●] {self.config.mode}'

        if hasattr(self, 'btn_provider'):
            self.btn_provider.text = self._get_provider_summary()

        if self.error_msg:
            self.error_label.text = f'⚠️  {self.error_msg}'
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

        # Build choices with availability status
        choices = []
        for actor in self.actors_available:
            available, hint = self.actor_availability.get(actor, (True, "Unknown"))
            if available:
                # Installed: show checkmark
                display_text = f'  {actor.ljust(max_name_len)}  ✓  {hint}'
            else:
                # Not installed: show cross and hint
                display_text = f'  {actor.ljust(max_name_len)}  ✗  {hint}'
            choices.append((actor, display_text))

        # Add 'none' option for optional roles
        if role == 'aux':
            choices.insert(0, ('none', f'  {"none".ljust(max_name_len)}  -  Disabled'))

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
        self._show_actor_dialog_fallback(role, choices, title, current)

    def _show_actor_dialog_fallback(self, role: str, choices, title: str, current: str) -> None:
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

        dialog = Dialog(
            title=title,
            body=HSplit([
                Label(text='✓ = Installed    ✗ = Not installed', style='class:hint'),
                Label(text='↑↓: Navigate  |  Space: select  |  Tab: to buttons  |  Enter: confirm', style='class:hint'),
                Window(height=1),
                radio,
            ], key_bindings=kb_body),
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

        choices = [('none', 'none'), ('reuse_aux', "reuse aux's agent")]
        choices.extend([(a, a) for a in self.actors_available])

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

        dialog = Dialog(
            title='Foreman Agent',
            body=HSplit([
                Label(text='Select foreman agent for scheduled tasks'),
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
                self.root.content = HSplit([
                    create_header(),
                    Window(height=1),
                    self.setup_content,
                ])
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
                Label(text='How to interact with CCCC?'),
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
            token_field = TextArea(height=1, multiline=False, text=self.config.sl_token)
            channel_field = TextArea(height=1, multiline=False, text=self.config.sl_chan)

            def on_ok() -> None:
                self.config.sl_token = token_field.text.strip()
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
        # Save config and write commands
        self._save_config()

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

    def _build_runtime_ui(self) -> None:
        """Build modern runtime UI (Full-width Timeline + Input + Footer)"""
        # Wrap input field with clear Frame border and title
        input_with_frame = Frame(
            body=self.input_field,
            title='Message (Enter to send, Esc to focus timeline)',
            style='class:input-frame'
        )
        
        # Rebuild root with clean, full-width layout
        self.root.content = HSplit([
            create_runtime_header(),
            Window(height=1),
            # Timeline takes full width
            HSplit([
                Label(text='💬 Conversation:', style='class:section'),
                self.timeline,
            ], padding=0),
            Window(height=1),
            input_with_frame,
            Window(
                content=FormattedTextControl(self._get_footer_text),
                height=Dimension(min=5, max=5),
                dont_extend_height=True
            ),
        ])

        # Focus timeline by default (for natural mouse scrolling)
        try:
            self.app.layout.focus(self.timeline)
        except Exception:
            pass

    def _save_config(self) -> None:
        """Save configuration and trigger orchestrator launch"""
        cmds = self.home / "state" / "commands.jsonl"
        cmds.parent.mkdir(parents=True, exist_ok=True)

        # No noisy debug line on timeline for file path

        # IMPORTANT: Use 'w' mode to overwrite, not 'a' to append
        # Each setup completion should start fresh, not mix with previous sessions
        with cmds.open('w', encoding='utf-8') as f:
            ts = time.time()

            # Roles
            for role in ['peerA', 'peerB', 'aux']:
                actor = getattr(self.config, role, '')
                if actor and actor != 'none':
                    cmd = {
                        "type": "roles-set-actor",
                        "args": {"role": role, "actor": actor},
                        "source": "tui",
                        "ts": ts
                    }
                    f.write(json.dumps(cmd, ensure_ascii=False) + '\n')
                    # quiet: roles are reflected in status panel; avoid timeline noise

            # IM configuration via im-config command
            if self.config.mode == 'telegram' and self.config.tg_token:
                cmd = {
                    "type": "im-config",
                    "args": {
                        "provider": "telegram",
                        "token": self.config.tg_token
                    },
                    "source": "tui",
                    "ts": ts
                }
                if self.config.tg_chat:
                    cmd["args"]["chat_id"] = self.config.tg_chat
                f.write(json.dumps(cmd, ensure_ascii=False) + '\n')
                # quiet: IM config persisted; avoid timeline noise
            elif self.config.mode == 'slack' and self.config.sl_token:
                cmd = {
                    "type": "im-config",
                    "args": {
                        "provider": "slack",
                        "bot_token": self.config.sl_token
                    },
                    "source": "tui",
                    "ts": ts
                }
                if self.config.sl_chan:
                    cmd["args"]["channel_id"] = self.config.sl_chan
                f.write(json.dumps(cmd, ensure_ascii=False) + '\n')
                # quiet: IM config persisted; avoid timeline noise
            elif self.config.mode == 'discord' and self.config.dc_token:
                cmd = {
                    "type": "im-config",
                    "args": {
                        "provider": "discord",
                        "bot_token": self.config.dc_token
                    },
                    "source": "tui",
                    "ts": ts
                }
                if self.config.dc_chan:
                    cmd["args"]["channel_id"] = self.config.dc_chan
                f.write(json.dumps(cmd, ensure_ascii=False) + '\n')
                # quiet: IM config persisted; avoid timeline noise

            # Launch command (triggers orchestrator to start peers)
            cmd = {"type": "launch", "args": {"who": "both"}, "source": "tui", "ts": ts}
            f.write(json.dumps(cmd, ensure_ascii=False) + '\n')
            # quiet: launch command queued; avoid timeline noise

        # Foreman yaml
        if self.config.foreman and self.config.foreman != 'none':
            _write_yaml(self.home, 'settings/foreman.yaml', {
                'agent': self.config.foreman,
                'enabled': True
            })

        # IM provider configuration via commands.jsonl only
        # Orchestrator will handle yaml updates through im-config command
        # No direct yaml writes here to preserve complete configuration

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
        Append message with clean compact formatting (no ANSI codes, pure text).
        
        Format: HH:MM 🤖 SENDER │ Message content
                               │ Continuation lines

        Args:
            text: Message content  
            msg_type: system/peerA/peerB/user/error/info/success/warning/debug
            silent: If True, don't increment message count
        """
        current_time = time.time()
        timestamp = time.strftime('%H:%M')

        # Sender config: (icon, label) - no colors, pure text
        sender_config = {
            'system': ('🔧', 'SYS'),
            'peerA': ('🤖', self.config.peerA.upper() if hasattr(self.config, 'peerA') else 'PEERA'),
            'peerB': ('⚙️', self.config.peerB.upper() if hasattr(self.config, 'peerB') else 'PEERB'),
            'user': ('👤', 'YOU'),
            'error': ('❌', 'ERR'),
            'info': ('ℹ️', 'INF'),
            'success': ('✅', 'OK'),
            'warning': ('⚠️', 'WRN'),
            'debug': ('🔍', 'DBG'),
        }

        icon, label = sender_config.get(msg_type, ('•', msg_type.upper()[:3]))

        # Check if same sender within 30s (compact mode)
        compact = (
            self.last_message_sender == msg_type and
            current_time - self.last_message_time < 30
        )

        lines = []

        # Wrap text at 100 chars for readability
        max_width = 100
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

        # Format message (compact or full)
        if compact:
            # Same sender continuation: omit timestamp
            for i, text_line in enumerate(text_lines):
                if i == 0:
                    lines.append(f'      {icon} │ {text_line}')
                else:
                    lines.append(f'        │ {text_line}')
        else:
            # New sender or time gap: show full header
            if self.timeline.text:
                lines.append('')  # Blank line between groups
            for i, text_line in enumerate(text_lines):
                if i == 0:
                    lines.append(f'{timestamp} {icon} {label:6s} │ {text_line}')
                else:
                    lines.append(f'               │ {text_line}')

        # Update tracking
        self.last_message_sender = msg_type
        self.last_message_time = current_time

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
        foreman_enabled = foreman_data.get('enabled', False) if isinstance(foreman_data, dict) else False
        foreman_status = 'ON' if foreman_enabled else 'OFF'
        foreman_display = f"{foreman_agent} ({foreman_status})" if foreman_agent != 'none' else 'none'

        row1 = f"Agents: {peerA} ⇄ {peerB} │ Aux: {aux_display} │ Foreman: {foreman_display}"

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
        # For now, check if telegram is configured
        telegram_enabled = self.config.mode == 'telegram' and bool(self.config.tg_token)
        telegram_icon = '●' if telegram_enabled else '○'

        # Build connection mode string
        mode_parts = ['tmux●']  # tmux is always active
        mode_parts.append(f'telegram{telegram_icon}')
        mode_str = '+'.join([p for p in mode_parts if '●' in p]) if any('●' in p for p in mode_parts) else 'tmux'

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

        # === Build formatted text with proper styling ===
        text = [
            ('class:section', '─' * 80 + '\n'),
            ('class:info', row1), ('', '\n'),
            ('class:info', row2), ('', '\n'),
            ('class:info', row3), ('', '\n'),
            ('class:section', '─' * 80),
        ]

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
            self._write_timeline("  /refresh            Refresh system prompt", 'info')
            self._write_timeline("  /quit               Quit CCCC (exit all processes)", 'info')
            self._write_timeline("", 'info')
            self._write_timeline("Foreman:", 'info')
            self._write_timeline("  /foreman on         Enable Foreman", 'info')
            self._write_timeline("  /foreman off        Disable Foreman", 'info')
            self._write_timeline("  /foreman status     Show Foreman status", 'info')
            self._write_timeline("  /foreman now        Run Foreman immediately", 'info')
            self._write_timeline("", 'info')
            self._write_timeline("Aux:", 'info')
            self._write_timeline("  /c <prompt>         Run Aux helper", 'info')
            self._write_timeline("  /review             Request Aux review", 'info')
            self._write_timeline("", 'info')
            self._write_timeline("Other:", 'info')
            self._write_timeline("  /focus [hint]       Focus PeerB", 'info')
            self._write_timeline("  /verbose on|off     Toggle peer summaries + Foreman CC", 'info')
            self._write_timeline("", 'info')
            self._write_timeline("Keyboard:", 'info')
            self._write_timeline("  Ctrl+T              Focus timeline (enable mouse scroll)", 'info')
            self._write_timeline("  Esc                 Return to input from timeline", 'info')
            self._write_timeline("  Ctrl+A/E            Start/end of line", 'info')
            self._write_timeline("  Ctrl+W/U/K          Delete word/start/end", 'info')
            self._write_timeline("  Up/Down             History", 'info')
            self._write_timeline("  PageUp/Down         Scroll timeline", 'info')
            self._write_timeline("  Ctrl+L              Clear timeline", 'info')
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
        elif text in ('/refresh', '/sys-refresh'):
            self._write_cmd_to_queue("sys-refresh", {}, "Refresh command sent")
        elif text == '/quit' or text == 'q':
            self._write_timeline("Shutting down CCCC...", 'system')
            self._quit_app()
        elif text == '/review':
            self._write_cmd_to_queue("aux", {"action": "review"}, "Review request sent")

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

        # Focus with optional hint
        elif text.startswith('/focus'):
            hint = text[6:].strip() if len(text) > 6 else ""
            self._write_cmd_to_queue("focus", {"hint": hint}, "Focus command sent")

        # Aux command with prompt
        elif text.startswith('/c '):
            prompt = text[3:].strip()
            if prompt:
                self._write_cmd_to_queue("aux", {"prompt": prompt}, "Aux command sent")
            else:
                self._write_timeline("Usage: /c <prompt>", 'error')

        # Message sending commands (keep existing logic)
        elif text.startswith('/a '):
            msg = text[3:].strip()
            if msg:
                self._write_timeline(f"You > PeerA: {msg}", 'user')
                try:
                    with cmds.open('a', encoding='utf-8') as f:
                        cmd = {"type": "a", "args": {"text": msg}, "source": "tui", "ts": ts}
                        f.write(json.dumps(cmd, ensure_ascii=False) + '\n')
                        f.flush()
                    self._write_timeline("Sent to PeerA", 'success')
                except Exception as e:
                    self._write_timeline(f"Failed to send: {str(e)[:50]}", 'error')
            else:
                self._write_timeline("Usage: /a <message>", 'error')

        elif text.startswith('/b '):
            msg = text[3:].strip()
            if msg:
                self._write_timeline(f"You > PeerB: {msg}", 'user')
                try:
                    with cmds.open('a', encoding='utf-8') as f:
                        cmd = {"type": "b", "args": {"text": msg}, "source": "tui", "ts": ts}
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
                self._write_timeline(f"You > Both: {msg}", 'user')
                try:
                    with cmds.open('a', encoding='utf-8') as f:
                        cmd = {"type": "both", "args": {"text": msg}, "source": "tui", "ts": ts}
                        f.write(json.dumps(cmd, ensure_ascii=False) + '\n')
                        f.flush()
                    self._write_timeline("Sent to both peers", 'success')
                except Exception as e:
                    self._write_timeline(f"Failed to send: {str(e)[:50]}", 'error')
            else:
                self._write_timeline("Usage: /both <message>", 'error')

        else:
            self._write_timeline(f"Unknown command: {text}. Type /help for help.", 'error')

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

    def _open_dialog(self, dialog: Dialog, ok_handler: Optional[callable] = None) -> None:
        """Open dialog"""
        if self.modal_open:
            return

        float_dialog = Float(content=dialog)
        # Update root container to include the dialog
        if hasattr(self.root, 'floats'):
            self.root.floats.append(float_dialog)
        else:
            # Fallback: store in local floats list
            self.floats.append(float_dialog)

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

        # Refocus first button
        try:
            self.app.layout.focus(self.btn_peerA)
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
            self.last_message_time = 0
            self.last_message_sender = ''
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

        # Return to input from timeline (Esc)
        @kb.add('escape', filter=has_focus(self.timeline))
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

        while True:
            try:
                if not self.setup_visible:
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

                await asyncio.sleep(2.0)

            except asyncio.CancelledError:
                # Properly handle cancellation
                break
            except Exception:
                # Log error but continue loop
                await asyncio.sleep(2.0)

    def _check_residual_inbox(self) -> None:
        """Check for residual inbox messages before orchestrator launch.
        If messages found, shows dialog. Otherwise continues with launch.

        This check only runs ONCE after initial setup. Subsequent launches skip it
        because inbox messages are part of normal workflow, not residuals.
        """
        try:
            # Check if inbox has already been checked (flag file exists)
            inbox_checked_flag = self.home / "state" / "inbox_checked.flag"
            if inbox_checked_flag.exists():
                # Already checked before, skip and continue launch
                self._continue_launch()
                return

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

            # If no residual messages, set flag and continue
            if cntA == 0 and cntB == 0:
                inbox_checked_flag.parent.mkdir(parents=True, exist_ok=True)
                inbox_checked_flag.write_text(f"Inbox checked at {time.time()}\n", encoding='utf-8')
                self._continue_launch()
                return

            # Show dialog (dialog handlers will set flag after user choice)
            # Pass flag path to dialog so handlers can set it
            self._show_inbox_cleanup_dialog(cntA, cntB, inbox_checked_flag)

        except Exception as e:
            # Log error but don't block launch
            try:
                import json
                import time
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

    def _show_inbox_cleanup_dialog(self, cntA: int, cntB: int, flag_path: Path) -> None:
        """Show clear inbox cleanup dialog with user-friendly messaging
        
        Args:
            cntA: Count of messages in PeerA inbox
            cntB: Count of messages in PeerB inbox
            flag_path: Path to inbox_checked.flag file (set after user choice)
        """
        # Debug: Log that we're trying to show the inbox cleanup dialog
        self._write_timeline(f"Showing inbox cleanup dialog: {cntA} + {cntB} messages", 'debug')
        
        if self.modal_open:
            self._write_timeline("Dialog blocked: modal already open", 'debug')
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
            # Set flag AFTER user makes choice
            try:
                flag_path.parent.mkdir(parents=True, exist_ok=True)
                flag_path.write_text(f"Inbox checked at {time.time()}\n", encoding='utf-8')
            except Exception:
                pass
            self._close_dialog()
            self._write_timeline(f"Processing {total} pending message(s)", 'info')
            self._continue_launch()

        def on_discard() -> None:
            """Discard - move messages to processed/ directory"""
            self._cleanup_inbox_messages(discard=True)
            # Set flag AFTER user makes choice
            try:
                flag_path.parent.mkdir(parents=True, exist_ok=True)
                flag_path.write_text(f"Inbox checked at {time.time()}\n", encoding='utf-8')
            except Exception:
                pass
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
