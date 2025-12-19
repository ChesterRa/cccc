# -*- coding: utf-8 -*-
"""
API Error Recovery - Detect API errors and trigger peer restart

Purpose: When a peer's CLI process is alive but API calls are failing
         (400, 429, 500, etc.), detect the error patterns in pane output
         and trigger automatic restart.

Trigger: Pane output contains API error patterns (rate limit, status codes, etc.)
Behavior: After detecting error, restart peer if within restart limits
         Debounce prevents repeated restarts for same error (60s cooldown)

Design:
- Complements crash detection (which only detects dead panes)
- Reuses existing restart infrastructure (count_recent_restarts, restart_peer)
- Lightweight: only scans last 50 lines of pane output
- Notifies user via outbox when restart occurs or limit reached
"""
from __future__ import annotations
import re
import time
from typing import Any, Dict, Callable

# API error patterns that indicate peer needs restart
_API_ERROR_PATTERNS = [
    r'\berror\s*:?\s*40[0-9]\b',      # error 400-409 (client errors)
    r'\berror\s*:?\s*50[0-9]\b',      # error 500-509 (server errors)
    r'\bstatus\s*:?\s*40[0-9]\b',     # status: 400-409
    r'\bstatus\s*:?\s*50[0-9]\b',     # status: 500-509
    r'\brate.?limit\w*\b',            # rate limit / rate-limit / ratelimit
    r'\bAPIError\b',                  # Generic API error
    r'\boverloaded\b',                # Model overloaded
    r'\bAuthenticationError\b',       # Auth error
    r'\bInvalidRequestError\b',       # Invalid request
    r'\bRateLimitError\b',            # Rate limit error class
    r'\bServiceUnavailable\b',        # Service unavailable
]
_API_ERROR_COMPILED = [re.compile(p, re.IGNORECASE) for p in _API_ERROR_PATTERNS]

# Debounce settings
_DEBOUNCE_SEC = 60.0  # Min seconds between API error restarts for same peer


def make(ctx: Dict[str, Any]):
    """
    Create API error recovery API.

    Required ctx keys:
    - home: Path to .cccc directory
    - tmux: function to run tmux commands
    - log_ledger: function to log events
    - outbox_write: function to notify user
    - count_recent_restarts: function to count recent restarts
    - restart_peer: function to restart a peer
    - auto_restart_enabled: bool
    - auto_restart_max_attempts: int
    - auto_restart_window_sec: int

    Returns dict with:
    - detect_api_error(text) -> bool
    - check_and_recover(peer_label, pane) -> bool
    """
    home = ctx['home']
    tmux = ctx['tmux']
    log_ledger = ctx['log_ledger']
    outbox_write = ctx['outbox_write']
    count_recent_restarts = ctx['count_recent_restarts']
    restart_peer = ctx['restart_peer']
    auto_restart_enabled = ctx.get('auto_restart_enabled', True)
    auto_restart_max_attempts = ctx.get('auto_restart_max_attempts', 3)
    auto_restart_window_sec = ctx.get('auto_restart_window_sec', 600)

    # Track last API error detection time per peer (debounce)
    _last_detected: Dict[str, float] = {}

    def detect_api_error(text: str) -> bool:
        """Check if text contains API error patterns."""
        for pattern in _API_ERROR_COMPILED:
            if pattern.search(text):
                return True
        return False

    def _should_check(peer_label: str) -> bool:
        """Check if enough time has passed since last check (debounce)."""
        now = time.time()
        last = _last_detected.get(peer_label, 0.0)
        return (now - last) >= _DEBOUNCE_SEC

    def _mark_detected(peer_label: str) -> None:
        """Mark that we detected an error for this peer (for debounce)."""
        _last_detected[peer_label] = time.time()

    def check_and_recover(peer_label: str, pane: str) -> bool:
        """
        Check pane output for API errors and restart if needed.

        Returns True if restart was triggered, False otherwise.
        """
        if not auto_restart_enabled:
            return False

        if not _should_check(peer_label):
            return False

        try:
            # Capture recent output (last 50 lines for efficiency)
            rc, content, _ = tmux("capture-pane", "-t", pane, "-p", "-S", "-50")
            if rc != 0 or not content:
                return False

            if not detect_api_error(content):
                return False

            # API error detected - mark for debounce
            _mark_detected(peer_label)

            # Check restart limits
            recent_restarts = count_recent_restarts(peer_label, auto_restart_window_sec)
            if recent_restarts >= auto_restart_max_attempts:
                print(f"[API-ERROR] {peer_label} restart limit reached")
                try:
                    outbox_write(home, {
                        "type": "to_user",
                        "peer": "System",
                        "text": f"🚨 {peer_label} API errors detected but restart limit reached. Please check API status and restart manually with /restart {peer_label.lower()}."
                    })
                except Exception:
                    pass
                return False

            # Attempt restart
            print(f"[API-ERROR] {peer_label} API error detected, attempting restart (attempt {recent_restarts + 1}/{auto_restart_max_attempts})")
            log_ledger(home, {
                "from": "system",
                "kind": "peer-api-error-detected",
                "peer": peer_label,
                "pane": pane
            })

            success = restart_peer(peer_label, reason="api-error")
            if success:
                try:
                    outbox_write(home, {
                        "type": "to_user",
                        "peer": "System",
                        "text": f"⚠️ {peer_label} API error detected, auto-restarted."
                    })
                except Exception:
                    pass
                return True
            else:
                print(f"[API-ERROR] {peer_label} restart failed")
                return False

        except Exception as e:
            # Silently continue on any error
            return False

    return {
        'detect_api_error': detect_api_error,
        'check_and_recover': check_and_recover,
    }
