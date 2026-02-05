"""Processing state tracker for CCCC daemon.

This module tracks whether actors are actively processing messages and detects
when they might need a reminder to use MCP tools for responses.

Strategy: Route B - Pure MCP-based detection (runtime agnostic)
- No PTY content capture or terminal activity monitoring
- Detection relies solely on MCP activity tracking and timeouts
- Compatible with all MCP-supporting runtimes (Claude Code, Codex, Gemini CLI, etc.)

Key behaviors:
1. When a message is delivered to an actor, mark them as PROCESSING
2. When actor calls cccc_message_send/reply, clear PROCESSING state
3. When actor calls any other MCP tool, refresh activity time (prevents false positives)
4. When actor has been PROCESSING too long + no recent MCP activity, nudge them

This prevents the common mistake where AI agents respond in terminal output
instead of using MCP tools, which means their responses never reach the user.
"""
from __future__ import annotations

import logging
import threading
from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum
from typing import Any, Callable, Dict, Optional, Set

logger = logging.getLogger("cccc.processing_state")


class ProcessingStatus(Enum):
    """Actor processing status."""
    IDLE = "idle"           # Not processing any message
    PROCESSING = "processing"  # Processing a message, waiting for MCP reply
    STALE = "stale"         # Processing too long, may need nudge


@dataclass
class ProcessingStateConfig:
    """Configuration for processing state tracking."""
    # Master switch
    enabled: bool = True

    # Timeout configuration
    processing_timeout_sec: float = 30.0    # Time before considering stale
    mcp_activity_grace_sec: float = 60.0    # Grace period after any MCP activity
    stale_giveup_sec: float = 300.0         # Give up nudging after this time

    # Nudge configuration
    nudge_enabled: bool = True
    nudge_max_count: int = 3
    nudge_base_interval_sec: float = 30.0
    nudge_interval_multiplier: float = 2.0

    # Detection configuration
    check_interval_sec: float = 5.0
    require_terminal_activity: bool = False  # Disabled: use pure MCP-based detection

    # MCP tool classification
    reply_tools: Set[str] = field(default_factory=lambda: {
        "cccc_message_send",
        "cccc_message_reply",
        "cccc_file_send",
    })


@dataclass
class ActorProcessingState:
    """Processing state for a single actor."""
    actor_id: str
    status: ProcessingStatus = ProcessingStatus.IDLE

    # Current message being processed
    current_message_id: Optional[str] = None
    processing_started_at: Optional[datetime] = None

    # MCP activity tracking (key improvement: track ALL MCP calls)
    last_mcp_activity_at: Optional[datetime] = None
    last_mcp_tool: Optional[str] = None
    mcp_call_count: int = 0
    active_mcp_calls: int = 0  # Number of MCP calls currently in progress

    # Terminal activity tracking
    terminal_activity_detected: bool = False
    last_terminal_activity_at: Optional[datetime] = None

    # Nudge tracking
    nudge_count: int = 0
    last_nudge_at: Optional[datetime] = None

    # Statistics
    total_messages_processed: int = 0
    total_nudges_sent: int = 0

    def reset(self) -> None:
        """Reset to idle state."""
        self.status = ProcessingStatus.IDLE
        self.current_message_id = None
        self.processing_started_at = None
        self.terminal_activity_detected = False
        self.last_terminal_activity_at = None
        self.nudge_count = 0
        self.last_nudge_at = None
        self.mcp_call_count = 0
        self.active_mcp_calls = 0


# Type for the stale callback
StaleCallback = Callable[[str, str, ActorProcessingState], None]


class ProcessingStateTracker:
    """
    Tracks actor processing state and detects when actors may need nudging.

    Responsibilities:
    1. Track per-actor message processing state
    2. Detect stale state (timeout + terminal activity + no MCP reply)
    3. Trigger nudge callback when stale

    Does NOT:
    - Send actual nudges (delegates to callback)
    - Deliver messages (called by DeliveryManager)
    - Handle MCP requests (called by MCP server)
    """

    def __init__(
        self,
        config: Optional[ProcessingStateConfig] = None,
        on_stale: Optional[StaleCallback] = None,
    ):
        self._config = config or ProcessingStateConfig()
        self._on_stale = on_stale
        self._states: Dict[tuple[str, str], ActorProcessingState] = {}  # (group_id, actor_id) -> state
        self._lock = threading.Lock()

    @property
    def config(self) -> ProcessingStateConfig:
        """Get current configuration."""
        return self._config

    def update_config(self, config: ProcessingStateConfig) -> None:
        """Update configuration."""
        with self._lock:
            self._config = config

    # ============================================================
    # State Operations (called externally)
    # ============================================================

    def on_message_delivered(
        self, group_id: str, actor_id: str, message_id: str
    ) -> None:
        """Called when a message is delivered to an actor's PTY."""
        if not self._config.enabled:
            return

        with self._lock:
            key = (group_id, actor_id)
            state = self._states.setdefault(key, ActorProcessingState(actor_id=actor_id))

            # Reset state for new message
            state.status = ProcessingStatus.PROCESSING
            state.current_message_id = message_id
            state.processing_started_at = datetime.now(timezone.utc)
            state.terminal_activity_detected = False
            state.last_terminal_activity_at = None
            state.nudge_count = 0
            state.last_nudge_at = None
            state.mcp_call_count = 0
            state.last_mcp_activity_at = None

            logger.debug(f"[{group_id}/{actor_id}] Processing started: {message_id}")

    def on_mcp_call_start(
        self, group_id: str, actor_id: str, tool_name: str
    ) -> None:
        """Called when an MCP call starts (before execution)."""
        if not self._config.enabled:
            return

        with self._lock:
            key = (group_id, actor_id)
            state = self._states.get(key)
            if state and state.status == ProcessingStatus.PROCESSING:
                state.active_mcp_calls += 1
                state.last_mcp_activity_at = datetime.now(timezone.utc)
                state.last_mcp_tool = tool_name
                state.mcp_call_count += 1
                logger.debug(
                    f"[{group_id}/{actor_id}] MCP call start: {tool_name} "
                    f"(active: {state.active_mcp_calls})"
                )

    def on_mcp_call_end(
        self, group_id: str, actor_id: str, tool_name: str
    ) -> None:
        """Called when an MCP call ends (after execution, success or failure)."""
        if not self._config.enabled:
            return

        with self._lock:
            key = (group_id, actor_id)
            state = self._states.get(key)
            if state and state.active_mcp_calls > 0:
                state.active_mcp_calls -= 1
                state.last_mcp_activity_at = datetime.now(timezone.utc)
                logger.debug(
                    f"[{group_id}/{actor_id}] MCP call end: {tool_name} "
                    f"(active: {state.active_mcp_calls})"
                )

    def on_mcp_activity(
        self, group_id: str, actor_id: str, tool_name: str
    ) -> None:
        """Called when actor makes any MCP call (except reply tools).

        DEPRECATED: Use on_mcp_call_start/end for better tracking.
        Kept for backward compatibility.
        """
        if not self._config.enabled:
            return

        with self._lock:
            key = (group_id, actor_id)
            state = self._states.get(key)
            if state and state.status == ProcessingStatus.PROCESSING:
                state.last_mcp_activity_at = datetime.now(timezone.utc)
                state.last_mcp_tool = tool_name
                state.mcp_call_count += 1
                logger.debug(
                    f"[{group_id}/{actor_id}] MCP activity: {tool_name} "
                    f"(count: {state.mcp_call_count})"
                )

    def on_mcp_reply(self, group_id: str, actor_id: str) -> None:
        """Called when actor sends a message via MCP (send/reply)."""
        if not self._config.enabled:
            return

        with self._lock:
            key = (group_id, actor_id)
            state = self._states.get(key)
            if state and state.status != ProcessingStatus.IDLE:
                state.total_messages_processed += 1
                state.reset()
                logger.debug(f"[{group_id}/{actor_id}] Processing completed via MCP reply")

    def on_terminal_activity(self, group_id: str, actor_id: str) -> None:
        """Called when terminal output is detected for an actor."""
        if not self._config.enabled:
            return

        with self._lock:
            key = (group_id, actor_id)
            state = self._states.get(key)
            if state and state.status == ProcessingStatus.PROCESSING:
                state.terminal_activity_detected = True
                state.last_terminal_activity_at = datetime.now(timezone.utc)

    def on_actor_removed(self, group_id: str, actor_id: str) -> None:
        """Called when an actor is removed - clean up state."""
        with self._lock:
            self._states.pop((group_id, actor_id), None)

    def on_actor_restart(self, group_id: str, actor_id: str) -> None:
        """Called when an actor restarts - reset state."""
        with self._lock:
            key = (group_id, actor_id)
            state = self._states.get(key)
            if state:
                state.reset()

    # ============================================================
    # Query Interface
    # ============================================================

    def get_state(self, group_id: str, actor_id: str) -> Optional[ActorProcessingState]:
        """Get actor state (for debugging/monitoring)."""
        with self._lock:
            return self._states.get((group_id, actor_id))

    def get_all_states(self) -> Dict[tuple[str, str], ActorProcessingState]:
        """Get all states (for debugging)."""
        with self._lock:
            return dict(self._states)

    def is_processing(self, group_id: str, actor_id: str) -> bool:
        """Check if actor is currently processing a message."""
        with self._lock:
            state = self._states.get((group_id, actor_id))
            return state is not None and state.status == ProcessingStatus.PROCESSING

    # ============================================================
    # Periodic Check (called by daemon tick)
    # ============================================================

    def tick(self) -> None:
        """Called periodically to check all actor states."""
        if not self._config.enabled:
            return

        now = datetime.now(timezone.utc)
        stale_actors: list[tuple[str, str, ActorProcessingState]] = []

        with self._lock:
            for (group_id, actor_id), state in list(self._states.items()):
                if self._check_if_stale(group_id, state, now):
                    state.status = ProcessingStatus.STALE
                    stale_actors.append((group_id, actor_id, state))

        # Trigger callbacks outside lock
        for group_id, actor_id, state in stale_actors:
            self._trigger_nudge(group_id, actor_id, state)

    def _check_if_stale(self, group_id: str, state: ActorProcessingState, now: datetime) -> bool:
        """Check if actor state should be considered stale."""
        # 1. Must be in PROCESSING status
        if state.status != ProcessingStatus.PROCESSING:
            return False

        # 2. Must have started processing
        if state.processing_started_at is None:
            return False

        elapsed = (now - state.processing_started_at).total_seconds()

        # 3. Must exceed base timeout
        if elapsed < self._config.processing_timeout_sec:
            return False

        # 4. Check if we should give up
        if elapsed > self._config.stale_giveup_sec:
            logger.warning(
                f"[{group_id}/{state.actor_id}] Giving up after {elapsed:.0f}s, "
                f"{state.nudge_count} nudges"
            )
            state.reset()
            return False

        # 5. Check max nudges
        if state.nudge_count >= self._config.nudge_max_count:
            return False

        # 6. Check nudge interval
        if state.last_nudge_at:
            nudge_interval = self._config.nudge_base_interval_sec * (
                self._config.nudge_interval_multiplier ** state.nudge_count
            )
            if (now - state.last_nudge_at).total_seconds() < nudge_interval:
                return False

        # 7. KEY: Check for active MCP calls (prevents false positives during long-running tools)
        if state.active_mcp_calls > 0:
            # MCP call in progress - actor is working, don't nudge
            return False

        # 8. Check for recent MCP activity (grace period after call ends)
        if state.last_mcp_activity_at:
            mcp_elapsed = (now - state.last_mcp_activity_at).total_seconds()
            if mcp_elapsed < self._config.mcp_activity_grace_sec:
                # Recent MCP activity - actor is working, don't nudge
                return False

        # 9. Must have terminal activity (if required)
        if self._config.require_terminal_activity and not state.terminal_activity_detected:
            return False

        # 10. Check terminal activity timing vs MCP activity
        # If terminal activity happened before last MCP activity, it's tool output
        if state.last_terminal_activity_at and state.last_mcp_activity_at:
            if state.last_terminal_activity_at <= state.last_mcp_activity_at:
                return False

        return True

    def _trigger_nudge(
        self, group_id: str, actor_id: str, state: ActorProcessingState
    ) -> None:
        """Trigger a nudge for a stale actor."""
        with self._lock:
            state.nudge_count += 1
            state.last_nudge_at = datetime.now(timezone.utc)
            state.total_nudges_sent += 1

        logger.info(
            f"[{group_id}/{actor_id}] Triggering nudge #{state.nudge_count} "
            f"(message: {state.current_message_id})"
        )

        if self._on_stale:
            try:
                self._on_stale(group_id, actor_id, state)
            except Exception as e:
                logger.exception(f"Error in on_stale callback: {e}")

    # ============================================================
    # Debug Interface
    # ============================================================

    def debug_summary(self, group_id: Optional[str] = None) -> Dict[str, Any]:
        """Return debug summary of all states."""
        with self._lock:
            result: Dict[str, Any] = {
                "config": {
                    "enabled": self._config.enabled,
                    "processing_timeout_sec": self._config.processing_timeout_sec,
                    "mcp_activity_grace_sec": self._config.mcp_activity_grace_sec,
                    "nudge_enabled": self._config.nudge_enabled,
                    "nudge_max_count": self._config.nudge_max_count,
                },
                "actors": {},
            }

            for (gid, aid), state in self._states.items():
                if group_id and gid != group_id:
                    continue

                key = f"{gid}/{aid}"
                result["actors"][key] = {
                    "status": state.status.value,
                    "current_message_id": state.current_message_id,
                    "processing_started_at": (
                        state.processing_started_at.isoformat()
                        if state.processing_started_at else None
                    ),
                    "last_mcp_activity_at": (
                        state.last_mcp_activity_at.isoformat()
                        if state.last_mcp_activity_at else None
                    ),
                    "last_mcp_tool": state.last_mcp_tool,
                    "mcp_call_count": state.mcp_call_count,
                    "active_mcp_calls": state.active_mcp_calls,
                    "terminal_activity_detected": state.terminal_activity_detected,
                    "nudge_count": state.nudge_count,
                    "total_messages_processed": state.total_messages_processed,
                    "total_nudges_sent": state.total_nudges_sent,
                }

            return result


# Global tracker instance (initialized by daemon)
TRACKER: Optional[ProcessingStateTracker] = None


def get_tracker() -> Optional[ProcessingStateTracker]:
    """Get the global tracker instance."""
    return TRACKER


def init_tracker(
    config: Optional[ProcessingStateConfig] = None,
    on_stale: Optional[StaleCallback] = None,
) -> ProcessingStateTracker:
    """Initialize the global tracker instance."""
    global TRACKER
    TRACKER = ProcessingStateTracker(config=config, on_stale=on_stale)
    return TRACKER
