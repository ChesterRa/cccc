"""Daemon-side federation: remote-send transports, dispatch/outbox and ops.

Stage 2 scope is transport adapters, an idempotent dispatch seam and the
``remote_send`` / ``remote_delivery_status`` daemon ops. No auto retry worker,
no multi-registry, no web/UI.
"""
