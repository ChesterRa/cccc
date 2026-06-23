"""Daemon-side group_bridge: remote-send transports, dispatch/outbox and ops.

Stage 2 scope is transport adapters, an idempotent dispatch seam and the
``remote_send`` / ``remote_delivery_status`` daemon ops, plus the lightweight
remote outbox retry worker. No multi-registry or web/UI logic lives here.
"""
